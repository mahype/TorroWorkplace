//! Agents & API (task A.6 in `docs/tasks/api-mcp.md`, concept in `docs/api.md`, "Rechte" and
//! "Steuerzentrale: Abschnitt „Agents & API“"): every program that may use the API – the suite's own
//! assistant and its bots, coding agents, hooks and scripts (task S.21) – and what each of them may
//! do.
//!
//! - **List:** one settings group per kind of client (the assistant, bots, agents, hooks, scripts),
//!   a row per client with its name, what it is, its rights in words ([`rights_summary`]) and when
//!   it was last used; the row opens the detail page. Without clients of the other kinds an empty
//!   state that points to the header's "Connect program", which opens the assistant of task A.7
//!   (`agent_wizard.rs`). "Connect program" and not "Connect agent" (task S.21): the section also
//!   holds the suite's own assistant and its bots, which no such dialog creates.
//! - **Detail:** the client's rights in the rights editor of `torro-settings-ui`
//!   ([`rights`], task S.33): levels, admin switches, the commands of an area one by one ("Show
//!   commands", named as MCP tools for agents and as endpoints for scripts, [`naming`]), "Send on my
//!   behalf", "Ask first" and the selections. Every other control of the page ("Create new key", a
//!   hook's fixed rights) starts in the editor's column ([`rights::control_column`]). Then its
//!   latest requests from the audit log (command and result, never contents); "Create new key" and
//!   "Disconnect…", each behind a confirmation that says what goes ([`disconnect_text`]). A new key
//!   is shown once, with a copy button.
//! - **The assistant** (`api::ASSISTANT_CLIENT`, tasks C.6 and S.21) is a group of its own, led like
//!   every other program: its row opens a detail page with the same rights view. While it has no
//!   client the row says "Turn on assistant" and its page is the first start – "Start from"
//!   ([`Preset`]) and the rights it is offered, with "Turn on assistant" in the header, which sends
//!   `clients.create_client` with the chosen rights ([`Message::TurnOn`], task C.11). Its page ends
//!   with "Turn off assistant…" (`clients.remove_client`), never with a new key: the core runs the
//!   assistant's tools itself with the client's rights, so nobody ever needs its key, and the page
//!   must not show it ([`answered`] drops it).
//! - **Bots** (`api::BOT_CLIENT`, task B.3) are a group of their own; their rows open the same
//!   rights view for the bot's own client, which sets what each bot may use, and "Open chat" in the
//!   header leads to the bot's chat window in the assistant app
//!   (`assistant.open_window { bot }`, [`chat_button`]). A bot's page has **no** destructive
//!   button ([`Category::away_label`]): who a bot is – name,
//!   instructions, skills – is edited in the assistant app, and whether it exists at all is the
//!   assistant's too. The areas `assistant` and `bots` have no
//!   row for a bot or for the assistant itself – the core refuses them whatever the rights say
//!   (`api::loops_back`). For every other agent a short warning stands under `bots` (it would act
//!   with a bot's rights) and `assistant` (it would also read the conversations with bots) while
//!   they are on ([`rights::bot_warning`]).
//! - **Hook clients** (`api::HOOK_CLIENT`) show the agent they report for and their fixed rights,
//!   read-only: the core narrows a hook's rights to `Rights::hook` whatever is sent, because its key
//!   sits in files the agent can read itself.
//!
//! Every change edits a draft of the client's rights, which the page shows at once;
//! [`rights::SAVE_DELAY`] after the last change (`docs/settings.md`: autosave after 500 ms) it goes
//! out as `clients.update_client` with the whole rights. The editor keeps the draft; the page hands
//! it every answer and reload first ([`answered`], [`loaded`]), so no control jumps back for a
//! moment. The assistant's first start changes rights no client holds yet ([`rights::change`]).
//!
//! "Disconnect…" also takes Torro Workplace out of the agent's configuration when the assistant
//! put it there: once the core confirmed `clients.remove_client`, the shell runs
//! [`Job::Disconnect`], which looks up what the assistant noted for that client and does nothing
//! for any other. A failure shows above the list. The agent's inbox hooks go with it (task E2.3):
//! the job takes them out of the agent's settings and names their hook client, which the page
//! then removes from the core as well (`clients.remove_client`, one after the other).
//! Disconnecting a hook client itself takes out only its hooks.
//!
//! Requests: `clients.update_client` (every control of the rights), `clients.activity` (opening a
//! detail page), `clients.new_key`, `clients.remove_client` ("Disconnect…", "Turn off assistant…"),
//! `clients.create_client` ("Turn on assistant" and the "Connect program" dialog) and
//! `assistant.open_window` ("Open chat" on a bot's page). The clients
//! come from
//! `clients.list_clients`, the candidates of the selections from `calendar.list_calendars`,
//! `tasks.list_lists` (whose GitHub filter lists also name repositories), `inbox.list_sources` and
//! `accounts.list_accounts` (the accounts of the kind `mail`), all loaded by the shell into [`Data`]
//! and handed to the editor as [`rights::Known`]. Only the app key may send `clients.*`; the control
//! centre has it. Until task S.35 splits this section into API and MCP, the editor lives here in the
//! control centre.

use chrono::{DateTime, Local, Utc};
use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{Space, center, column, container, responsive, row, svg, text_input};
use iced::{Alignment, Element, Fill, Length, Subscription, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Status};
use torro_gui::{style, typo};
use torro_inbox::AgentKind;
use torro_ipc::api::{ASSISTANT_CLIENT, Area, BOT_CLIENT, HOOK_CLIENT, Level, Rights, RightsTemplate};
use torro_ipc::{ActivityEntry, ClientInfo, IssuedKey, Request, Response};
use torro_settings_ui::rights::{self, Naming, control_column, in_column, rights_summary, template_label};
// The assistant's tool lines name areas the way the rights editor does (issue #133).
pub(crate) use torro_settings_ui::rights::area_label;
#[cfg(test)]
pub(super) use torro_settings_ui::rights::sends_freely;

use super::accounts::relative_time;
use super::{Action, Data, Header, agent_wizard, chevron, confirm, dialog, pressable, row_frame, setting_row};
#[cfg(test)]
use crate::client_setup::Released;
use crate::client_setup::{Job, Outcome};

/// The latest requests the detail page shows; the core keeps 50.
const ACTIVITY_SHOWN: usize = 10;

/// The name of the assistant's client, as the list and the audit log show it (decision 5 of
/// `docs/assistent.md`: one client "Assistant").
pub const CLIENT_NAME: &str = "Assistant";

/// Page state.
#[derive(Debug)]
pub struct State {
    /// The client whose detail page is open (its `ClientInfo::id`); `None` shows the list.
    pub selected: Option<String>,
    /// The assistant's page is open (task S.21): its client's detail page, or its first start while
    /// it has no client. Kept apart from `selected`, which names a client the core lists – the
    /// first start has none yet.
    assistant: bool,
    /// What the assistant's first start offers until "Turn on assistant": [`Rights::assistant`],
    /// then what the person changed.
    choice: Rights,
    /// "Turn on assistant" was sent and the core's list does not show the client yet.
    creating: bool,
    /// The rights editor of the detail page and of the assistant's first start: its draft on the
    /// way to the core, the typed repository, the open areas.
    rights: rights::State,
    /// The client whose activity was asked for and not answered yet.
    asked: Option<String>,
    /// The latest requests of a client, as `clients.activity` answered for it.
    activity: Option<(String, Vec<ActivityEntry>)>,
    /// The confirmation that is open.
    dialog: Option<Dialog>,
    /// A key the core just handed out, shown once until "Done".
    issued: Option<Issued>,
    /// The "Connect agent" dialog while it is open.
    pub wizard: Option<agent_wizard::State>,
    /// The client whose removal was sent; once the core confirms it, its configuration entry goes.
    removing: Option<String>,
    /// Why taking Torro Workplace out of an agent's configuration failed, shown above the list.
    problem: Option<String>,
    /// Hook clients of a disconnected agent still to remove from the core.
    orphans: Vec<String>,
}

impl Default for State {
    fn default() -> Self {
        State {
            selected: None,
            assistant: false,
            choice: Rights::assistant(),
            creating: false,
            rights: rights::State::default(),
            asked: None,
            activity: None,
            dialog: None,
            issued: None,
            wizard: None,
            removing: None,
            problem: None,
            orphans: Vec::new(),
        }
    }
}

/// A starting point of the assistant's first start (task C.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// [`Rights::assistant`]: read calendars, tasks and the Notification Hub.
    Suggested,
    Template(RightsTemplate),
}

impl Preset {
    const ALL: [Preset; 4] = [
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

    fn label(self) -> &'static str {
        match self {
            Preset::Suggested => tr("Suggested"),
            Preset::Template(template) => template_label(template),
        }
    }

    fn detail(self) -> &'static str {
        match self {
            Preset::Suggested => tr("Reads calendars, tasks and the Notification Hub"),
            Preset::Template(template) => agent_wizard::template_detail(template),
        }
    }
}

/// The confirmations of the detail page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    /// "Create a new key?": the old key stops working.
    NewKey,
    /// "Disconnect this agent?"
    Disconnect,
}

/// A key just handed out by `clients.new_key`. It lives only in this window's memory and only
/// until "Done": the core cannot tell it again.
#[derive(Debug)]
struct Issued {
    /// The client's name, for the dialog title.
    name: String,
    key: IssuedKey,
    /// "Copy" was pressed; the button says "Copied".
    copied: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Opens the detail page of a client.
    Open(String),
    /// The assistant's row in the list: its detail page, or its first start while it has no client
    /// (task S.21).
    ShowAssistant,
    /// A starting point of the assistant's first start.
    Preset(Preset),
    /// "Turn on assistant" in the header of the first start: `clients.create_client` with the
    /// chosen rights.
    TurnOn,
    /// Back from a detail page to the list.
    Back,
    /// A control of the rights editor, its clock included ([`subscription`]).
    Rights(rights::Message),
    /// "Create new key", then its confirmation.
    AskNewKey,
    NewKey,
    /// "Copy" in the dialog with the new key.
    CopyKey,
    /// "Done" (or Escape) in the dialog with the new key: the key is gone for good.
    CloseKey,
    /// Typing into the key's field or a click next to its dialog: nothing happens, so the key
    /// stays readable and is not lost by a stray click.
    KeepKey,
    /// "Disconnect…", then its confirmation.
    AskDisconnect,
    Disconnect,
    /// "Open chat" in the header of a bot's page: the bot's chat window in the assistant app
    /// (`assistant.open_window` with the bot's id, not its client's).
    OpenChat(String),
    /// "Cancel" or Escape in a confirmation.
    CloseDialog,
    /// "Connect program" in the header: opens the assistant.
    Connect,
    Wizard(agent_wizard::Message),
    /// A job the shell ran for the page ([`Action::Setup`]): detection and writing for the
    /// assistant, removing an entry after "Disconnect…".
    Setup(Outcome),
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Open(id) => {
            state.assistant = false;
            open(state, id)
        }
        Message::Back => {
            state.selected = None;
            state.assistant = false;
            state.dialog = None;
            // Leaving the page is a good moment to save; the clock would do it a moment later.
            from_editor(rights::save_now(&mut state.rights))
        }
        // On, the assistant is a client like any other; off, its page is the first start, which
        // has no client to ask for activity.
        Message::ShowAssistant => {
            state.assistant = true;
            match assistant_client(data) {
                Some(client) => open(state, client.id.clone()),
                None => {
                    state.selected = None;
                    state.dialog = None;
                    rights::leave(&mut state.rights);
                    Action::None
                }
            }
        }
        Message::Preset(preset) => {
            if assistant_client(data).is_none() {
                state.choice = preset.rights();
            }
            Action::None
        }
        Message::TurnOn => {
            if assistant_client(data).is_some() || state.creating {
                return Action::None;
            }
            state.creating = true;
            // The chosen rights in the same request (task C.11), so the client never holds other
            // ones, not even for a moment; the template is the narrowest, in case an older core
            // ignores them.
            Action::Send(Request::CreateClient {
                name: CLIENT_NAME.into(),
                kind: ASSISTANT_CLIENT.into(),
                template: RightsTemplate::ReadOnly,
                agent: None,
                rights: Some(state.choice.clone()),
            })
        }
        Message::Rights(message) => edit(state, data, message),
        Message::AskNewKey => ask(state, data, Dialog::NewKey),
        Message::NewKey => {
            state.dialog = None;
            // Only a client that has a key of its own: the assistant and the bots run inside the
            // core, which holds theirs ([`Category::has_key`]), and a new one would only break them.
            match selected_client(state, data).filter(|client| Category::of(client).has_key()) {
                Some(client) => Action::Send(Request::NewClientKey { id: client.id.clone() }),
                None => Action::None,
            }
        }
        Message::CopyKey => match state.issued.as_mut() {
            Some(issued) => {
                issued.copied = true;
                Action::Copy(issued.key.expose().to_owned())
            }
            None => Action::None,
        },
        Message::CloseKey => {
            state.issued = None;
            Action::None
        }
        Message::KeepKey => Action::None,
        // Only a page that offers the button asks the question: a bot's has none
        // ([`Category::away_label`]), and its client must not be taken away from here.
        Message::AskDisconnect => match selected_client(state, data).map(Category::of) {
            Some(category) if category.away_label().is_some() => ask(state, data, Dialog::Disconnect),
            _ => Action::None,
        },
        Message::Disconnect => {
            state.dialog = None;
            let Some(id) = selected_client(state, data)
                .filter(|client| Category::of(client).away_label().is_some())
                .map(|client| client.id.clone())
            else {
                return Action::None;
            };
            let was_assistant = state.assistant;
            state.selected = None;
            // Turned off, the assistant starts from the suggestion again, as its first start does.
            if was_assistant {
                state.assistant = false;
                state.choice = Rights::assistant();
            }
            // Its rights need not be saved any more.
            rights::forget(&mut state.rights, &id);
            state.removing = Some(id.clone());
            Action::Send(Request::RemoveClient { id })
        }
        Message::OpenChat(bot) => {
            Action::Send(Request::OpenChatWindow { window: torro_ipc::assistant::ChatWindow::Bot(bot) })
        }
        Message::CloseDialog => {
            state.dialog = None;
            Action::None
        }
        Message::Connect => {
            state.wizard = Some(agent_wizard::State::new());
            state.problem = None;
            Action::Setup(Job::Detect)
        }
        Message::Wizard(message) => wizard(state, message, data),
        Message::Setup(Outcome::Disconnected(result)) => match result {
            Ok(released) => {
                state.problem = None;
                state.orphans.extend(released.hook_clients);
                remove_orphan(state)
            }
            Err(problem) => {
                state.problem = Some(problem);
                Action::None
            }
        },
        Message::Setup(outcome) => match state.wizard.as_mut() {
            Some(wizard) => agent_wizard::update(wizard, agent_wizard::Message::Setup(outcome), data),
            None => Action::None,
        },
    }
}

/// Opens the detail page of the client `id`: its latest requests come with it.
fn open(state: &mut State, id: String) -> Action {
    state.selected = Some(id.clone());
    state.dialog = None;
    rights::leave(&mut state.rights);
    state.activity = None;
    state.asked = Some(id.clone());
    Action::Send(Request::ClientActivity { id })
}

/// The assistant's client as the core lists it; `None` before it was turned on.
pub fn assistant_client(data: &Data) -> Option<&ClientInfo> {
    data.clients.iter().find(|client| client.kind == ASSISTANT_CLIENT)
}

/// Removes the next hook client of a disconnected agent, the way "Disconnect…" removes a client:
/// once the core confirms, its hooks are looked up too ([`answered`]), and the next one follows.
fn remove_orphan(state: &mut State) -> Action {
    if state.removing.is_some() {
        return Action::None;
    }
    match state.orphans.pop() {
        Some(id) => {
            state.removing = Some(id.clone());
            Action::Send(Request::RemoveClient { id })
        }
        None => Action::None,
    }
}

/// A message of the assistant: Cancel, Done and Escape close it unless it is creating the client,
/// a click next to it only on the steps that hold nothing to lose.
fn wizard(state: &mut State, message: agent_wizard::Message, data: &Data) -> Action {
    let Some(open) = state.wizard.as_mut() else { return Action::None };
    let close = match message {
        agent_wizard::Message::Close => agent_wizard::closable(open),
        agent_wizard::Message::Dismiss => agent_wizard::dismissable(open),
        message => return agent_wizard::update(open, message, data),
    };
    if close {
        state.wizard = None;
    }
    Action::None
}

/// Opens `dialog` for the client on the detail page; without one there is nothing to confirm.
fn ask(state: &mut State, data: &Data, dialog: Dialog) -> Action {
    state.dialog = selected_client(state, data).map(|_| dialog);
    Action::None
}

/// Hands a control of the rights editor to it: on the assistant's first start, where there is no
/// client yet, the change goes to the choice "Turn on assistant" will send ([`rights::change`]);
/// otherwise to the client on the detail page, saved [`rights::SAVE_DELAY`] later
/// ([`rights::update`], which also leaves a hook client's fixed rights alone and runs the clock).
fn edit(state: &mut State, data: &Data, message: rights::Message) -> Action {
    let known = known(data);
    if first_start(state, data) {
        rights::change(&mut state.rights, &mut state.choice, ASSISTANT_CLIENT, message, &known);
        return Action::None;
    }
    let client = selected_client(state, data);
    from_editor(rights::update(&mut state.rights, message, client, &known))
}

/// What the editor's selections offer: what the shell loaded into [`Data`].
fn known(data: &Data) -> rights::Known<'_> {
    rights::Known {
        calendars: &data.calendars,
        task_lists: &data.task_lists,
        sources: &data.sources,
        clients: &data.clients,
        accounts: &data.accounts,
    }
}

/// The editor's action as the shell's: it only ever sends `clients.update_client`.
fn from_editor(action: torro_settings_ui::Action<()>) -> Action {
    match action {
        torro_settings_ui::Action::Send(mut requests) if requests.len() == 1 => {
            requests.pop().map_or(Action::None, Action::Send)
        }
        torro_settings_ui::Action::Send(requests) => Action::SendAll(requests),
        _ => Action::None,
    }
}

/// How the editor names the commands of a client of `kind`: every MCP client, the assistant and a
/// bot use tools; a script calls the API's endpoints, as does a kind the suite does not know, which
/// the core checks like a script.
fn naming(kind: &str) -> Naming {
    match kind {
        ASSISTANT_CLIENT | BOT_CLIENT => Naming::Tools,
        kind if torro_ipc::api::is_mcp_kind(kind) => Naming::Tools,
        _ => Naming::Endpoints,
    }
}

/// The answer to a request the shell sent (it calls this for every answer, then reloads):
///
/// - the assistant's own answers ([`agent_wizard::answered`]): its new client's key,
/// - `ok` while the draft is on its way: saved, it goes with the next reload ([`loaded`]); an error
///   then drops it, so the page shows what the core holds and the shell shows the error.
/// - the activity asked for when the detail page opened,
/// - a new key ([`Response::ClientKey`]): shown once,
/// - `ok` after "Disconnect…": the client is gone, so its configuration entry goes too
///   ([`Job::Disconnect`]); an error forgets the removal.
///
/// Returns `Some` with what to do next when the answer was the assistant's (the shell then shows
/// no notice of its own; the assistant shows the error) or confirmed a removal; `None` otherwise.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> Option<Action> {
    if let Some(next) = state.wizard.as_mut().and_then(|wizard| agent_wizard::answered(wizard, result)) {
        return Some(next);
    }
    let for_draft = rights::answered(&mut state.rights, result);
    if !for_draft && state.removing.is_some() {
        match result {
            Ok(Response::Ok) => return state.removing.take().map(|client| Action::Setup(Job::Disconnect { client })),
            Err(_) => state.removing = None,
            Ok(_) => {}
        }
    }
    match result {
        Ok(Response::Activity { entries }) => {
            if let Some(id) = state.asked.take() {
                state.activity = Some((id, entries.clone()));
            }
        }
        // The assistant's key is dropped: the core runs its tools itself with the client's rights,
        // so nobody needs it, and no page of the suite ever shows it (task C.6).
        Ok(Response::ClientKey { client, .. }) if client.kind == ASSISTANT_CLIENT => {}
        Ok(Response::ClientKey { client, key }) => {
            state.issued = Some(Issued { name: client.name.clone(), key: key.clone(), copied: false });
        }
        Err(_) if state.creating => state.creating = false,
        _ => {}
    }
    None
}

/// The shell loaded [`Data`] anew: a saved draft is in it now. "Turn on assistant" is done once the
/// core lists the client; the assistant's first start then becomes that client's detail page, which
/// asks for its latest requests like any other (the returned action).
pub fn loaded(state: &mut State, data: &Data) -> Option<Action> {
    rights::loaded(&mut state.rights);
    let client = assistant_client(data)?;
    state.creating = false;
    if !state.assistant || state.selected.as_deref() == Some(client.id.as_str()) {
        return None;
    }
    Some(open(state, client.id.clone()))
}

/// The editor's clock that saves a waiting draft; none while nothing waits, so the page costs
/// nothing at rest.
pub fn subscription(state: &State) -> Subscription<Message> {
    rights::subscription(&state.rights).map(Message::Rights)
}

/// What Escape means on this page right now: close the assistant (unless it is creating the
/// client), the key dialog or the confirmation.
pub fn escape(state: &State) -> Option<Message> {
    if let Some(wizard) = &state.wizard {
        return agent_wizard::closable(wizard).then_some(Message::Wizard(agent_wizard::Message::Close));
    }
    if state.issued.is_some() {
        Some(Message::CloseKey)
    } else if state.dialog.is_some() {
        Some(Message::CloseDialog)
    } else {
        None
    }
}

/// The header bar: on a detail page the client's name with a back button to the list, and on a
/// bot's page "Open chat" ([`chat_button`]); on the assistant's first start its name and "Turn on
/// assistant" – the window's one primary button, where it stays in sight while the rights below
/// are chosen; on the list "Connect program". Only the icon of a primary button in narrow windows.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Header<'a, Message> {
    if let Some(client) = selected_client(state, data) {
        let actions = chat_button(data, client, class).into_iter().collect();
        return Header { sub_page: Some((client.name.clone(), Message::Back)), actions };
    }
    if first_start(state, data) {
        let button = match state.creating {
            true => widgets::busy_button(tr("Turning on…"), style::button::primary),
            false => primary(class, tr("Turn on assistant"), Message::TurnOn),
        };
        return Header { sub_page: Some((tr("Assistant").to_string(), Message::Back)), actions: vec![button] };
    }
    Header { sub_page: None, actions: vec![primary(class, tr("Connect program"), Message::Connect)] }
}

/// "Open chat" on a bot's page: its chat window in the assistant app, where the bot itself is
/// edited. `None` for every other client, and for a bot whose own entry the core no longer lists –
/// then there is nothing to chat with. In narrow windows only the icon, like the bot's page in the
/// control centre had it before task S.21.
fn chat_button<'a>(data: &'a Data, client: &ClientInfo, class: SizeClass) -> Option<Element<'a, Message>> {
    if client.kind != BOT_CLIENT {
        return None;
    }
    let bot = data.bots.iter().find(|bot| bot.client == client.id)?;
    let open = Some(Message::OpenChat(bot.id.clone()));
    Some(match class {
        SizeClass::Wide | SizeClass::Medium => {
            widgets::text_button(Some(Icon::ArrowUpRight), tr("Open chat"), style::button::secondary, open)
        }
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
            widgets::compact_button(Icon::ArrowUpRight, tr("Open chat"), style::button::secondary, open)
        }
    })
}

/// The header's primary button: with its word, in narrow windows only its "+" icon, so the section
/// menu keeps its room.
fn primary<'a>(class: SizeClass, label: &'a str, message: Message) -> Element<'a, Message> {
    match class {
        SizeClass::Wide | SizeClass::Medium => {
            widgets::text_button(Some(Icon::Plus), label, style::button::primary, Some(message))
        }
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
            widgets::compact_button(Icon::Plus, label, style::button::primary, Some(message))
        }
    }
}

/// The page's dialog over the whole window: the assistant, the new key, else a confirmation.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(wizard) = &state.wizard {
        let content = agent_wizard::view(wizard, data).map(Message::Wizard);
        return Some(dialog(content, size::form_max(), Message::Wizard(agent_wizard::Message::Dismiss)));
    }
    if let Some(issued) = &state.issued {
        return Some(key_dialog(issued));
    }
    let client = selected_client(state, data)?;
    let name: &dyn std::fmt::Display = &client.name;
    Some(match state.dialog? {
        Dialog::NewKey => confirm(
            tr("Create a new key?"),
            trf(
                "The current key of {name} stops working at once. Put the new key into its configuration.",
                &[("name", name)],
            ),
            tr("Create new key"),
            Message::NewKey,
            Message::CloseDialog,
        ),
        Dialog::Disconnect => {
            let (title, consequence) = disconnect_text(client);
            let word = Category::of(client).confirm_word();
            confirm(title, consequence, word, Message::Disconnect, Message::CloseDialog)
        }
    })
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The client whose detail page is open, if it is still listed. While the assistant's page is open
/// it is the assistant's client, whatever `selected` still holds: turned off, the page becomes the
/// first start, not the page of a client that is gone.
fn selected_client<'a>(state: &State, data: &'a Data) -> Option<&'a ClientInfo> {
    if state.assistant {
        return assistant_client(data);
    }
    let id = state.selected.as_deref()?;
    data.clients.iter().find(|client| client.id == id)
}

/// The assistant's first start is on screen: its page is open and it has no client yet.
fn first_start(state: &State, data: &Data) -> bool {
    state.assistant && assistant_client(data).is_none()
}

/// The rights the page shows for `client`: the editor's draft while there is one, else what the
/// core holds.
fn shown_rights<'a>(state: &'a State, client: &'a ClientInfo) -> &'a Rights {
    rights::shown(&state.rights, client)
}

/// "Claude Code", "Script", "Claude Code hooks": what a client is.
pub fn kind_label(client: &ClientInfo) -> String {
    match client.kind.as_str() {
        "claude-desktop" => "Claude Desktop".to_string(),
        "cursor" => "Cursor".to_string(),
        "script" => tr("Script").to_string(),
        ASSISTANT_CLIENT => tr("Assistant").to_string(),
        BOT_CLIENT => tr("Bot").to_string(),
        HOOK_CLIENT => match client.agent {
            Some(agent) => trf("{agent} hooks", &[("agent", &agent.label())]),
            None => tr("Hooks").to_string(),
        },
        kind => AgentKind::from_id(kind).map_or_else(|| kind.to_string(), |agent| agent.label().to_string()),
    }
}

/// What a client is, for the line under its name; `None` where that says nothing new: a script
/// sits in the group "Scripts", and "Claude Code" needs no "Claude Code" below it.
fn kind_word(client: &ClientInfo) -> Option<String> {
    let kind = kind_label(client);
    let named_by_group = matches!(Category::of(client), Category::Script | Category::Bot | Category::Assistant);
    (!named_by_group && kind != client.name).then_some(kind)
}

/// "Used 5 min ago", or "Never used".
pub fn last_used_text(last_used: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    match last_used {
        Some(at) => trf("Used {when}", &[("when", &relative_time(at, now))]),
        None => tr("Never used").to_string(),
    }
}

/// The line under the name on the detail page: what kind of client it is, what it is where the
/// name does not say so, and when it was last used ("Agent · Used 2 days ago", "Agent · Codex ·
/// Never used").
pub fn about(client: &ClientInfo, now: DateTime<Utc>) -> String {
    let mut parts = vec![Category::of(client).singular().to_string()];
    parts.extend(kind_word(client));
    parts.push(last_used_text(client.last_used, now));
    parts.join(" · ")
}

/// Title and consequence of the page's last button for `client`. An agent loses its key and, where
/// "Connect program" set them up, its configuration entry and inbox hooks ([`Job::Disconnect`]);
/// hooks are taken out of the agent's settings; a script only loses its key; the assistant is
/// turned off and keeps its conversations.
fn disconnect_text(client: &ClientInfo) -> (&'static str, String) {
    let name: &dyn std::fmt::Display = &client.name;
    match Category::of(client) {
        Category::Assistant => (
            tr("Turn off the assistant?"),
            tr(
                "It loses access to everything at once. Your conversations stay. Turn it on again to choose what it may use.",
            )
            .to_string(),
        ),
        Category::Agent => (
            tr("Disconnect this agent?"),
            trf(
                "{name} loses access at once: its key stops working. Where Connect agent set them up, its configuration entry and its Notification Hub hooks are removed too.",
                &[("name", name)],
            ),
        ),
        Category::Hook => (
            tr("Disconnect these hooks?"),
            trf(
                "{name} stop reporting at once: their key stops working and they are taken out of the agent's settings.",
                &[("name", name)],
            ),
        ),
        Category::Bot => (
            tr("Take this bot's access away?"),
            trf(
                "{name} loses its access at once. Saving the bot again in the assistant gives it a new one, without any rights.",
                &[("name", name)],
            ),
        ),
        Category::Script => (
            tr("Disconnect this script?"),
            trf(
                "{name} loses access at once: its key stops working. Connect it again to give it access.",
                &[("name", name)],
            ),
        ),
    }
}

/// The groups of the list: the assistant, its bots, agents, hooks, scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    /// The suite's own assistant (task C.6), one client of its kind.
    Assistant,
    /// The assistant's bots (task B.3).
    Bot,
    Agent,
    Hook,
    Script,
}

impl Category {
    const ALL: [Category; 5] = [Category::Assistant, Category::Bot, Category::Agent, Category::Hook, Category::Script];

    fn of(client: &ClientInfo) -> Category {
        match client.kind.as_str() {
            ASSISTANT_CLIENT => Category::Assistant,
            HOOK_CLIENT => Category::Hook,
            BOT_CLIENT => Category::Bot,
            "script" => Category::Script,
            _ => Category::Agent,
        }
    }

    /// One client of the group, for the line under its name on the detail page.
    fn singular(self) -> &'static str {
        match self {
            Category::Assistant => tr("Assistant"),
            Category::Agent => tr("Agent"),
            Category::Bot => tr("Bot"),
            Category::Hook => tr("Hook"),
            Category::Script => tr("Script"),
        }
    }

    fn title(self) -> &'static str {
        match self {
            Category::Assistant => tr("Assistant"),
            Category::Bot => tr("Bots"),
            Category::Agent => tr("Agents"),
            Category::Hook => tr("Hooks"),
            Category::Script => tr("Scripts"),
        }
    }

    fn footer(self) -> &'static str {
        match self {
            Category::Assistant => {
                tr("The assistant chats with you and works in Torro Workplace with only what you allow it here.")
            }
            Category::Agent => {
                tr("Coding agents and apps that use Torro Workplace through MCP, each with its own key.")
            }
            Category::Bot => tr("The assistant's bots, each with rights of its own. Open one to set what it may use."),
            Category::Hook => {
                tr("Hooks report to the Notification Hub what an agent waits for or has finished. They see nothing.")
            }
            Category::Script => tr("Scripts and other programs that use the API with their own key."),
        }
    }

    /// The last button of the detail page: what it takes away, in the words of what it is; `None`
    /// for a page that has none.
    ///
    /// A bot has none. Taking its client away cannot be undone from here – the core refuses
    /// `clients.create_client` for the kind `bot`, so only the next `bots.save_bot` in the
    /// assistant would give the bot a client again, with no rights. Task S.21 names one
    /// destructive button for this section, "Turn off assistant…", and the bot's page in the
    /// control centre never had one. What a bot may use is set here; whether it exists at all is
    /// the assistant's.
    fn away_label(self) -> Option<&'static str> {
        match self {
            Category::Assistant => Some(tr("Turn off assistant…")),
            Category::Bot => None,
            Category::Agent | Category::Hook | Category::Script => Some(tr("Disconnect…")),
        }
    }

    /// The word on the button of its confirmation.
    fn confirm_word(self) -> &'static str {
        match self {
            Category::Assistant => tr("Turn off"),
            Category::Bot => tr("Take away"),
            Category::Agent | Category::Hook | Category::Script => tr("Disconnect"),
        }
    }

    /// Whether a client of this kind has a key a person puts into a configuration. The assistant
    /// and the bots run inside the core, which holds their keys: a new one would only break them,
    /// so their pages offer none.
    fn has_key(self) -> bool {
        matches!(self, Category::Agent | Category::Hook | Category::Script)
    }
}

/// When a request was: "14:05" today, "yesterday 14:05", else "2026-10-09 14:05" (no month names,
/// so it reads the same in every language).
fn when(time: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (local, today) = (time.with_timezone(&Local), now.with_timezone(&Local).date_naive());
    let clock = local.format("%H:%M").to_string();
    match (today - local.date_naive()).num_days() {
        0 => clock,
        1 => trf("yesterday {time}", &[("time", &clock)]),
        _ => local.format("%Y-%m-%d %H:%M").to_string(),
    }
}

/// The result of a request from the audit log as status and word.
pub fn outcome(result: &str) -> (Status, String) {
    match result {
        "ok" => (Status::Ok, tr("Done").to_string()),
        "pending" => (Status::Attention, tr("Waiting").to_string()),
        "denied" => (Status::Inactive, tr("Denied").to_string()),
        "forbidden" => (Status::Error, tr("Not allowed").to_string()),
        "not-found" => (Status::Error, tr("Not found").to_string()),
        code => (Status::Error, trf("Failed: {code}", &[("code", &code)])),
    }
}

// ----- Views --------------------------------------------------------------------------------

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let now = Utc::now();
    match selected_client(state, data) {
        Some(client) => detail(state, client, data, now),
        None if first_start(state, data) => start(state, data),
        None => list(state, data, now),
    }
}

/// The list: the assistant's group – which is there whether it is on or off –, then one group per
/// other kind of client, or the empty state while there is no other client.
fn list<'a>(state: &'a State, data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
    let problem = state.problem.as_deref().map(problem_line);
    let groups: Vec<Element<'a, Message>> = Category::ALL
        .into_iter()
        .filter_map(|category| {
            let mut rows: Vec<Element<'a, Message>> = data
                .clients
                .iter()
                .filter(|client| Category::of(client) == category)
                .map(|client| client_row(state, client, now))
                .collect();
            // The assistant has a row even without a client: the one way to turn it on.
            if category == Category::Assistant && rows.is_empty() {
                rows.push(assistant_off_row());
            }
            (!rows.is_empty()).then(|| widgets::group(category.title(), rows, Some(category.footer().to_string())))
        })
        .collect();
    // Only the assistant's group: the page says where the other programs come from.
    let empty = (groups.len() == 1).then(|| {
        empty_state(
            tr("No programs connected"),
            tr(
                "Choose Connect program to set up Claude Code, Codex, Gemini CLI, Claude Desktop or Cursor, or to create a key for a script.",
            ),
        )
    });
    column![].push(problem).extend(groups).push(empty).spacing(layout::section_gap()).into()
}

/// The assistant's row while it has no client: off, and the one thing to do about it.
fn assistant_off_row<'a>() -> Element<'a, Message> {
    let row = setting_row(
        Some(widgets::status_dot(Status::Inactive)),
        tr("The assistant is off"),
        Some(tr("Turn on assistant").to_string()),
        chevron(),
    );
    pressable(row, Some(Message::ShowAssistant))
}

/// The empty list: the section's icon over what shows up here and how – `widgets::empty_state`
/// with an icon on top, as the inbox does it (`torroinbox::list`) until torro-gui offers one.
fn empty_state<'a>(title: &'a str, hint: &'a str) -> Element<'a, Message> {
    let mark =
        icon::icon(Icon::Sparkles, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let words = column![
        typo::text(typo::body_strong(), title).align_x(Alignment::Center),
        typo::text(typo::body(), hint).style(theme::text_2).align_x(Alignment::Center)
    ]
    .spacing(space::xs())
    .align_x(Alignment::Center);
    let content = column![mark, words].spacing(space::m()).align_x(Alignment::Center).max_width(size::form_max() / 2.0);
    center(content).padding(space::xl()).into()
}

/// Why an agent's configuration still holds its entry: an alert icon in the error colour and the
/// sentence in `text`, the colour only in the icon.
fn problem_line(problem: &str) -> Element<'_, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
    let text =
        trf("Torro Workplace could not be taken out of the agent's configuration: {problem}", &[("problem", &problem)]);
    row![alert, typo::text(typo::body(), text)].spacing(space::s()).align_y(Alignment::Center).into()
}

/// A client in the list: name, then what it is, its rights in words and when it was last used;
/// the row opens the detail page.
fn client_row<'a>(state: &State, client: &'a ClientInfo, now: DateTime<Utc>) -> Element<'a, Message> {
    let mut parts: Vec<String> = kind_word(client).into_iter().collect();
    parts.push(rights_summary(shown_rights(state, client), &client.kind));
    parts.push(last_used_text(client.last_used, now));
    let row = setting_row(None, client.name.as_str(), Some(parts.join(" · ")), chevron());
    // Every client, the assistant and the bots included, opens the same detail page (task S.21);
    // the assistant's keeps track of being the assistant's, so turning it off lands on its first
    // start instead of a page without a client.
    let open = match client.kind.as_str() {
        ASSISTANT_CLIENT => Message::ShowAssistant,
        _ => Message::Open(client.id.clone()),
    };
    pressable(row, Some(open))
}

/// The detail page in the width the page has ([`control_column`], the editor's column).
fn detail<'a>(state: &'a State, client: &'a ClientInfo, data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
    responsive(move |size| detail_page(state, client, data, now, control_column(size.width)))
        .height(Length::Shrink)
        .into()
}

/// The detail page: name, the rights (or a hook's fixed ones), the recent activity, the key and
/// "Disconnect…" at the end; the controls in `column` ([`control_column`]).
fn detail_page<'a>(
    state: &'a State,
    client: &'a ClientInfo,
    data: &'a Data,
    now: DateTime<Utc>,
    column: Option<f32>,
) -> Element<'a, Message> {
    let category = Category::of(client);
    let mut page = column![heading(client.name.as_str(), about(client, now))].spacing(layout::section_gap());
    if client.kind == HOOK_CLIENT {
        page = page.push(hook_group(client, column));
    } else {
        let rights = shown_rights(state, client);
        let title = access_title(category);
        let editor =
            rights::groups(&state.rights, rights, &client.kind, &known(data), naming(&client.kind), title, column);
        page = page.extend(editor.into_iter().map(|group| group.map(Message::Rights)));
    }
    page = page.push(activity_group(state, client, now));
    if category.has_key() {
        page = page.push(key_group(column));
    }
    // A bot's page ends without one: see [`Category::away_label`].
    if let Some(label) = category.away_label() {
        let away = widgets::text_button(None, label, style::button::danger, Some(Message::AskDisconnect));
        page = page.push(on_axis(away));
    }
    page.into()
}

/// The name of the rights group, in the words of what the client is: an agent is given access, the
/// assistant and a bot are allowed to use things.
fn access_title(category: Category) -> &'static str {
    match category {
        Category::Assistant => tr("What the assistant may use"),
        Category::Bot => tr("What the bot may use"),
        Category::Agent | Category::Hook | Category::Script => tr("Access"),
    }
}

/// The name of a detail page with the line under it.
fn heading<'a>(name: &'a str, about: String) -> Element<'a, Message> {
    let name = typo::text(typo::headline(), name).wrapping(Wrapping::None);
    let about = typo::text(typo::meta(), about).style(theme::text_2);
    column![container(name).width(Fill).clip(true), about].spacing(space::xs()).into()
}

/// The assistant's first start in the width the page has.
fn start<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    responsive(move |size| start_page(state, data, control_column(size.width))).height(Length::Shrink).into()
}

/// The assistant's first start: that it is off, the starting points of its rights ([`Preset`]) and
/// the rights themselves, which "Turn on assistant" in the header sends along with the client.
fn start_page<'a>(state: &'a State, data: &'a Data, column: Option<f32>) -> Element<'a, Message> {
    let about = format!("{} · {}", Category::Assistant.singular(), tr("Off"));
    let title = access_title(Category::Assistant);
    let editor =
        rights::groups(&state.rights, &state.choice, ASSISTANT_CLIENT, &known(data), Naming::Tools, title, column);
    column![heading(CLIENT_NAME, about), off_group(), preset_group(&state.choice)]
        .extend(editor.into_iter().map(|group| group.map(Message::Rights)))
        .spacing(layout::section_gap())
        .into()
}

/// The first start's head: the assistant is off and what to do about it.
fn off_group<'a>() -> Element<'a, Message> {
    let detail = tr("Choose what it may use below, then turn it on.");
    let row = setting_row(None, tr("The assistant is off"), Some(detail.to_string()), Space::new());
    let footer =
        tr("It chats with you and works in Torro Workplace with only what you allow here. You can change it any time.");
    widgets::group(tr("Status"), vec![row], Some(footer.to_string()))
}

/// The starting points of the first start, with a check at the one the rights equal.
fn preset_group<'a>(rights: &Rights) -> Element<'a, Message> {
    let rows = Preset::ALL
        .into_iter()
        .map(|preset| {
            let mark: Element<'a, Message> = match preset.rights() == *rights {
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

/// A text button moved left by its own padding, so its label stands on the page's content axis.
pub(super) fn on_axis<'a, M: 'a>(button: Element<'a, M>) -> Element<'a, M> {
    row![Space::new(), button].spacing(-style::button_padding().left).into()
}

/// A hook client: the agent it reports for and its fixed rights, read-only, with the reason.
fn hook_group<'a>(client: &'a ClientInfo, column: Option<f32>) -> Element<'a, Message> {
    let agent = client.agent.map_or(tr("No agent"), |agent| agent.label());
    let value =
        |text: &'a str| in_column(typo::text(typo::body(), text).style(theme::text_2).wrapping(Wrapping::None), column);
    let rows = vec![
        setting_row(None, tr("Reports for"), None, value(agent)),
        setting_row(
            None,
            area_label(Area::Inbox),
            Some(tr("Reports and resolves its own entries, sees none").to_string()),
            value(widgets::level_label(Level::Write)),
        ),
    ];
    let footer = tr(
        "A hook's key sits in files the agent can read itself, so it may only report to the Notification Hub and sees nothing else. These rights can't be changed.",
    );
    widgets::group(tr("Access"), rows, Some(footer.to_string()))
}

/// The latest requests: command, time and result. The log holds no contents, so neither does
/// the page.
fn activity_group<'a>(state: &'a State, client: &ClientInfo, now: DateTime<Utc>) -> Element<'a, Message> {
    let title = tr("Recent activity");
    let entries = match &state.activity {
        Some((id, entries)) if *id == client.id => entries,
        _ => return quiet_group(title, tr("Loading …")),
    };
    if entries.is_empty() {
        return quiet_group(title, tr("No requests yet."));
    }
    let rows = entries.iter().take(ACTIVITY_SHOWN).map(|entry| activity_row(entry, now)).collect();
    let footer = tr("Only the command and its result are logged, never what it contained.");
    widgets::group(title, rows, Some(footer.to_string()))
}

/// A group title with a quiet sentence instead of rows (an empty group would be a bare frame).
fn quiet_group<'a>(title: &'a str, sentence: &'a str) -> Element<'a, Message> {
    column![widgets::section_title(title), typo::text(typo::meta(), sentence).style(theme::text_3)]
        .spacing(space::s())
        .into()
}

/// A request: the command, below it the time in tabular figures (so the times of the rows line
/// up), and the result at the right. Laid out like a `setting_row` with a detail, whose detail
/// cannot be tabular.
fn activity_row(entry: &ActivityEntry, now: DateTime<Utc>) -> Element<'_, Message> {
    let (status, word) = outcome(&entry.result);
    let result = row![widgets::status_dot(status), typo::text(typo::meta(), word).style(theme::text_2)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    let lines = column![
        typo::text(typo::body(), entry.command.as_str()).wrapping(Wrapping::None),
        typo::text(typo::meta().tabular(), when(entry.time, now)).style(theme::text_2).wrapping(Wrapping::None),
    ];
    let text = container(lines).width(Fill).clip(true);
    row_frame(row![text, result].spacing(space::l()).align_y(Alignment::Center), true)
}

/// The key: where it lives, and "Create new key".
fn key_group<'a>(column: Option<f32>) -> Element<'a, Message> {
    let create = widgets::text_button(None, tr("Create new key"), style::button::secondary, Some(Message::AskNewKey));
    let detail = Some(tr("Only the agent's configuration holds it").to_string());
    let footer = tr("A new key replaces the old one at once.");
    let row = setting_row(None, tr("API key"), detail, in_column(create, column));
    widgets::group(tr("Key"), vec![row], Some(footer.to_string()))
}

/// The new key, shown this once: the key in a field to select, "Copy" and "Done". A click next to
/// the dialog does nothing, so a stray click does not lose the key.
fn key_dialog(issued: &Issued) -> Element<'_, Message> {
    // Tabular figures: a key is read and compared character by character.
    let field = text_input("", issued.key.expose())
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().tabular().font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        // Without an edit handler the field cannot take focus, so the key could not be selected.
        // Edits change nothing: the field always shows the key.
        .on_input(|_| Message::KeepKey);
    let copy = match issued.copied {
        true => widgets::text_button(Some(Icon::Check), tr("Copied"), style::button::secondary, Some(Message::CopyKey)),
        false => widgets::text_button(Some(Icon::Copy), tr("Copy"), style::button::secondary, Some(Message::CopyKey)),
    };
    let done = widgets::text_button(None, tr("Done"), style::button::primary, Some(Message::CloseKey));
    let content = column![
        typo::text(typo::title(), trf("New key for {name}", &[("name", &issued.name)])),
        typo::text(
            typo::body(),
            tr("Copy it into the agent's configuration now. It is shown only this once; Torro Workplace keeps only a fingerprint of it.")
        )
        .style(theme::text_2),
        row![field, copy].spacing(space::s()).align_y(Alignment::Center),
        row![blank::horizontal(), done],
    ]
    .spacing(space::l());
    dialog(content, size::form_max(), Message::KeepKey)
}

#[cfg(test)]
mod tests {
    use chrono::Duration as Age;
    use iced::time::Instant;
    use torro_inbox::{SourceInfo, SourceRef};
    use torro_ipc::api::Confirm;
    use torro_ipc::{AccountInfo, Key};
    use torro_pim::model::GitHubFilter;
    use torro_pim::{AccountState, Collection, CollectionKind};
    use torro_settings_ui::rights::Pick;

    use super::*;

    /// Task S.33: the detail page edits through the rights editor – a level and a command's
    /// switch end in one `clients.update_client` with the whole rights – and names the commands as
    /// tools for agents and as endpoints for scripts.
    #[test]
    fn the_detail_page_edits_through_the_rights_editor() {
        let data = data();
        let mut state = opened("c1", &data);
        let editor = Message::Rights;
        let _ = update(&mut state, editor(rights::Message::Expand(Area::Calendar)), &data);
        assert_eq!(subscription(&state).units(), 0, "opening an area changes no rights");
        drop(view(&state, &data));
        let _ = update(&mut state, editor(rights::Message::Level(Area::Calendar, Level::Read)), &data);
        let _ = update(&mut state, editor(rights::Message::Command("calendar.list_events", false)), &data);
        let (id, rights) = saved(&mut state, &data);
        assert_eq!(id, "c1");
        assert_eq!(rights.area(Area::Calendar).level, Level::Read);
        assert!(rights.denied.iter().eq(["calendar.list_events"]), "{:?}", rights.denied);
        assert_eq!(rights.area(Area::Tasks).level, Level::Full, "the rest unchanged");

        assert_eq!(naming("claude-code"), Naming::Tools);
        assert_eq!(naming(torro_ipc::api::MCP_CLIENT), Naming::Tools);
        assert_eq!(naming(ASSISTANT_CLIENT), Naming::Tools);
        assert_eq!(naming(BOT_CLIENT), Naming::Tools);
        assert_eq!(naming("script"), Naming::Endpoints);
        assert_eq!(naming("something-older"), Naming::Endpoints, "checked like a script by the core");
    }

    fn client(id: &str, name: &str, kind: &str, rights: Rights) -> ClientInfo {
        ClientInfo {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            created: Utc::now(),
            last_used: None,
            rights: rights.for_kind(kind),
            agent: (kind == HOOK_CLIENT).then_some(AgentKind::ClaudeCode),
        }
    }

    fn account(id: &str, kind: &str, label: &str) -> AccountInfo {
        AccountInfo {
            id: id.into(),
            kind: kind.into(),
            label: label.into(),
            state: AccountState::NotConnected,
            collections: Vec::new(),
            scopes: None,
            mail: None,
            ntfy: None,
            services: None,
        }
    }

    fn collection(id: &str, name: &str, kind: CollectionKind, queries: &[&str]) -> Collection {
        let source = (!queries.is_empty()).then(|| GitHubFilter {
            queries: queries.iter().map(|query| query.to_string()).collect(),
            pinned: Vec::new(),
            revision: 1,
            truncated: false,
            problem: None,
        });
        Collection { id: id.into(), name: name.into(), kind, color: Some("#5B8CFF".into()), writable: true, source }
    }

    fn data() -> Data {
        Data {
            clients: vec![
                client("c1", "Claude Code", "claude-code", RightsTemplate::ReadAndWrite.rights()),
                client("h1", "Claude Code hooks", HOOK_CLIENT, Rights::hook()),
                client("s1", "backup", "script", RightsTemplate::ReadOnly.rights()),
            ],
            calendars: vec![
                collection("acc/work", "Work", CollectionKind::Events, &[]),
                collection("acc/home", "Home", CollectionKind::Events, &[]),
            ],
            task_lists: vec![
                collection("acc/todo", "Todo", CollectionKind::Todos, &[]),
                collection(
                    "github:anna/1",
                    "Bugs",
                    CollectionKind::Todos,
                    &["is:open repo:anna/torro label:bug -repo:anna/old", "org:acme is:pr"],
                ),
            ],
            sources: vec![SourceInfo { source: SourceRef::script("nightly"), open: 1, muted: false, health: None }],
            accounts: vec![
                account("nextcloud:anna@cloud.example.org", "nextcloud", "anna@cloud.example.org"),
                account("mail:anna@mailbox.org", "mail", "anna@mailbox.org"),
                account("", "mail", "signing-in@example.org"),
                account("mail:anna@work.example.net", "mail", "anna@work.example.net"),
            ],
            ..Data::default()
        }
    }

    /// The state with the detail page of `id` open.
    fn opened(id: &str, data: &Data) -> State {
        let mut state = State::default();
        let action = update(&mut state, Message::Open(id.into()), data);
        assert!(matches!(action, Action::Send(Request::ClientActivity { id: ref asked }) if asked == id));
        state
    }

    /// Runs the clock past the delay and returns the request it sends.
    fn saved(state: &mut State, data: &Data) -> (String, Rights) {
        let later = Instant::now() + rights::SAVE_DELAY + rights::SAVE_DELAY;
        match update(state, Message::Rights(rights::Message::Clock(later)), data) {
            Action::Send(Request::UpdateClient { id, rights }) => (id, rights),
            other => panic!("expected clients.update_client, got {other:?}"),
        }
    }

    #[test]
    fn back_saves_at_once_and_the_draft_shows_until_the_reload() {
        let data = data();
        let mut state = opened("c1", &data);
        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Apps, Level::Read)), &data);
        let client = &data.clients[0];
        assert_eq!(shown_rights(&state, client).area(Area::Apps).level, Level::Read, "shown at once");
        let action = update(&mut state, Message::Back, &data);
        assert!(matches!(action, Action::Send(Request::UpdateClient { ref id, .. }) if id == "c1"));
        answered(&mut state, &Ok(Response::Ok));
        assert_eq!(shown_rights(&state, client).area(Area::Apps).level, Level::Read, "until the reload");
        let _ = loaded(&mut state, &data);
        assert_eq!(shown_rights(&state, client).area(Area::Apps).level, Level::Full, "then what the core holds");
    }

    #[test]
    fn a_failed_save_drops_the_draft() {
        let data = data();
        let mut state = opened("c1", &data);
        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Apps, Level::Read)), &data);
        let _ = saved(&mut state, &data);
        answered(&mut state, &Err("forbidden".into()));
        assert_eq!(shown_rights(&state, &data.clients[0]).area(Area::Apps).level, Level::Full);
    }

    #[test]
    fn the_clock_runs_only_while_a_change_waits() {
        let data = data();
        let mut state = opened("c1", &data);
        assert_eq!(subscription(&state).units(), 0);
        let _ = update(&mut state, Message::Rights(rights::Message::Confirm(Confirm::Never)), &data);
        assert_eq!(subscription(&state).units(), 1);
        let _ = saved(&mut state, &data);
        assert_eq!(subscription(&state).units(), 0);
    }

    #[test]
    fn hook_rights_cannot_be_changed() {
        let data = data();
        let mut state = opened("h1", &data);
        for message in [
            Message::Rights(rights::Message::Level(Area::Calendar, Level::Full)),
            Message::Rights(rights::Message::Admin(Area::Accounts, true)),
            Message::Rights(rights::Message::All(Pick::InboxSources, true)),
        ] {
            let _ = update(&mut state, message, &data);
        }
        assert!(subscription(&state).units() == 0);
        assert_eq!(subscription(&state).units(), 0);
    }

    #[test]
    fn a_new_key_is_confirmed_shown_once_and_copied() {
        let data = data();
        let mut state = opened("s1", &data);
        assert!(matches!(update(&mut state, Message::AskNewKey, &data), Action::None));
        assert_eq!(state.dialog, Some(Dialog::NewKey));
        assert!(matches!(escape(&state), Some(Message::CloseDialog)));
        let action = update(&mut state, Message::NewKey, &data);
        assert!(matches!(action, Action::Send(Request::NewClientKey { ref id }) if id == "s1"));
        assert_eq!(state.dialog, None);

        let key = IssuedKey::new(Key::new("torro_s1_00ff"));
        answered(&mut state, &Ok(Response::ClientKey { client: data.clients[2].clone(), key }));
        assert!(overlay(&state, &data).is_some(), "the key dialog");
        assert!(matches!(update(&mut state, Message::KeepKey, &data), Action::None));
        assert!(state.issued.is_some(), "a click next to it keeps the key");
        let copy = update(&mut state, Message::CopyKey, &data);
        assert!(matches!(copy, Action::Copy(ref text) if text == "torro_s1_00ff"));
        assert!(state.issued.as_ref().is_some_and(|issued| issued.copied));
        assert!(matches!(escape(&state), Some(Message::CloseKey)));
        let _ = update(&mut state, Message::CloseKey, &data);
        assert!(state.issued.is_none() && overlay(&state, &data).is_none(), "gone for good");
    }

    #[test]
    fn disconnect_is_confirmed_and_removes_the_client() {
        let data = data();
        let mut state = opened("c1", &data);
        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Apps, Level::Read)), &data);
        let _ = update(&mut state, Message::AskDisconnect, &data);
        assert_eq!(state.dialog, Some(Dialog::Disconnect));
        assert!(overlay(&state, &data).is_some());
        let action = update(&mut state, Message::Disconnect, &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "c1"));
        assert!(
            state.selected.is_none() && subscription(&state).units() == 0,
            "back to the list, nothing left to save"
        );

        let mut cancelled = opened("c1", &data);
        let _ = update(&mut cancelled, Message::AskDisconnect, &data);
        let _ = update(&mut cancelled, Message::CloseDialog, &data);
        assert!(cancelled.dialog.is_none() && cancelled.selected.is_some());
    }

    #[test]
    fn connect_agent_opens_the_assistant_and_asks_for_detection() {
        let data = data();
        let mut state = State::default();
        assert_eq!(header(&state, &data, SizeClass::Wide).actions.len(), 1, "Connect agent");
        assert!(matches!(update(&mut state, Message::Connect, &data), Action::Setup(Job::Detect)));
        assert!(state.wizard.is_some() && overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::Wizard(agent_wizard::Message::Close))));
        let _ = update(&mut state, Message::Wizard(agent_wizard::Message::Dismiss), &data);
        assert!(state.wizard.is_none(), "a click next to the first step closes it");
        assert!(header_of_detail_has_no_actions(&data));
    }

    fn header_of_detail_has_no_actions(data: &Data) -> bool {
        let state = opened("c1", data);
        header(&state, data, SizeClass::Narrow).actions.is_empty()
    }

    #[test]
    fn disconnect_removes_the_configuration_entry_once_the_core_confirmed() {
        let data = data();
        let mut state = opened("c1", &data);
        let _ = update(&mut state, Message::AskDisconnect, &data);
        let _ = update(&mut state, Message::Disconnect, &data);
        let next = answered(&mut state, &Ok(Response::Ok));
        assert!(matches!(next, Some(Action::Setup(Job::Disconnect { ref client })) if client == "c1"));
        assert!(answered(&mut state, &Ok(Response::Ok)).is_none(), "once");

        let mut failed = opened("c1", &data);
        let _ = update(&mut failed, Message::Disconnect, &data);
        assert!(answered(&mut failed, &Err("not-found".into())).is_none(), "the shell shows the error");
        assert!(answered(&mut failed, &Ok(Response::Ok)).is_none(), "and the entry stays");

        let _ = update(&mut state, Message::Setup(Outcome::Disconnected(Err("~/.cursor/mcp.json".into()))), &data);
        assert!(state.problem.is_some());
        drop(view(&state, &data));
        drop(view(&state, &Data::default()));
        let _ = update(&mut state, Message::Setup(Outcome::Disconnected(Ok(Released::default()))), &data);
        assert!(state.problem.is_none());
    }

    #[test]
    fn disconnecting_an_agent_removes_its_hook_client_too() {
        let data = data();
        let mut state = opened("c1", &data);
        let _ = update(&mut state, Message::Disconnect, &data);
        assert!(matches!(answered(&mut state, &Ok(Response::Ok)), Some(Action::Setup(Job::Disconnect { .. }))));
        let released = Released { target: None, hook_clients: vec!["h1".into()] };
        let action = update(&mut state, Message::Setup(Outcome::Disconnected(Ok(released))), &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "h1"));
        // The core confirms: h1's own entries are looked up (nothing left), and nothing follows.
        let next = answered(&mut state, &Ok(Response::Ok));
        assert!(matches!(next, Some(Action::Setup(Job::Disconnect { ref client })) if client == "h1"));
        let last = update(&mut state, Message::Setup(Outcome::Disconnected(Ok(Released::default()))), &data);
        assert!(matches!(last, Action::None));
    }

    #[test]
    fn the_activity_belongs_to_the_client_it_was_asked_for() {
        let data = data();
        let mut state = opened("c1", &data);
        let entry = ActivityEntry {
            time: Utc::now(),
            client: Some("c1".into()),
            command: "calendar.list_events".into(),
            ids: Vec::new(),
            result: "ok".into(),
            approval: None,
        };
        answered(&mut state, &Ok(Response::Activity { entries: vec![entry] }));
        assert!(matches!(&state.activity, Some((id, entries)) if id == "c1" && entries.len() == 1));
        let _ = update(&mut state, Message::Back, &data);
        let _ = update(&mut state, Message::Open("s1".into()), &data);
        assert!(state.activity.is_none(), "another client's activity is not shown");
    }

    #[test]
    fn kinds_and_last_use_in_words() {
        let data = data();
        assert_eq!(kind_label(&data.clients[0]), "Claude Code");
        assert_eq!(kind_label(&data.clients[1]), trf("{agent} hooks", &[("agent", &"Claude Code")]));
        assert_eq!(kind_label(&data.clients[2]), tr("Script"));
        assert_eq!(kind_word(&data.clients[0]), None, "the name says it already");
        assert_eq!(kind_word(&data.clients[2]), None, "scripts have a group of their own");
        let mut renamed = data.clients[0].clone();
        renamed.name = "Laptop agent".into();
        assert_eq!(kind_word(&renamed).as_deref(), Some("Claude Code"));
        let now = Utc::now();
        assert_eq!(last_used_text(None, now), tr("Never used"));
        let minutes = trf("{n} min ago", &[("n", &5)]);
        assert_eq!(last_used_text(Some(now - Age::minutes(5)), now), trf("Used {when}", &[("when", &minutes)]));
    }

    #[test]
    fn outcomes_are_words_with_a_status() {
        assert_eq!(outcome("ok"), (Status::Ok, tr("Done").to_string()));
        assert_eq!(outcome("pending").0, Status::Attention);
        assert_eq!(outcome("denied").0, Status::Inactive);
        assert_eq!(outcome("conflict"), (Status::Error, trf("Failed: {code}", &[("code", &"conflict")])));
    }

    #[test]
    fn times_read_the_same_in_every_language() {
        let now = Utc::now();
        assert_eq!(when(now, now), now.with_timezone(&Local).format("%H:%M").to_string());
        let old = now - Age::days(9);
        assert_eq!(when(old, now), old.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string());
        let yesterday = now - Age::days(1);
        let clock = yesterday.with_timezone(&Local).format("%H:%M").to_string();
        assert_eq!(when(yesterday, now), trf("yesterday {time}", &[("time", &clock)]));
    }

    #[test]
    fn the_line_under_the_name_says_what_the_client_is() {
        let data = data();
        let now = Utc::now();
        let never = tr("Never used");
        assert_eq!(about(&data.clients[0], now), format!("{} · {never}", tr("Agent")));
        // Named as the suite names hooks in the language shown, so the kind adds nothing.
        let mut hook = data.clients[1].clone();
        hook.name = kind_label(&hook);
        assert_eq!(about(&hook, now), format!("{} · {never}", tr("Hook")));
        assert_eq!(about(&data.clients[2], now), format!("{} · {never}", tr("Script")));
        let mut renamed = data.clients[0].clone();
        renamed.name = "Laptop agent".into();
        assert_eq!(about(&renamed, now), format!("{} · Claude Code · {never}", tr("Agent")));
    }

    #[test]
    fn disconnect_says_what_goes_with_the_client() {
        let data = data();
        let titles: Vec<&str> = data.clients.iter().map(|client| disconnect_text(client).0).collect();
        assert_eq!(
            titles,
            [tr("Disconnect this agent?"), tr("Disconnect these hooks?"), tr("Disconnect this script?")]
        );
        let name: &dyn std::fmt::Display = &"Claude Code";
        let agent = trf(
            "{name} loses access at once: its key stops working. Where Connect agent set them up, its configuration entry and its Notification Hub hooks are removed too.",
            &[("name", name)],
        );
        assert_eq!(disconnect_text(&data.clients[0]).1, agent);
    }

    #[test]
    fn the_page_builds_in_every_state() {
        let data = data();
        let mut state = State::default();
        drop(view(&state, &data));
        drop(view(&state, &Data::default()));
        for id in ["c1", "h1", "s1"] {
            state = opened(id, &data);
            drop(view(&state, &data));
            for pick in Pick::ALL {
                let _ = update(&mut state, Message::Rights(rights::Message::All(pick, false)), &data);
            }
            drop(view(&state, &data));
            let _ = update(&mut state, Message::AskDisconnect, &data);
            assert!(overlay(&state, &data).is_some());
        }
    }

    /// Task S.21: while the assistant has no client, its row leads to the first start – "Start
    /// from" and the rights it is offered –, and "Turn on assistant" sends `clients.create_client`
    /// with exactly those rights (task C.11). Its key never reaches the page.
    #[test]
    fn the_assistant_is_turned_on_with_the_rights_of_its_first_start() {
        let data = data();
        let mut state = State::default();
        assert!(assistant_client(&data).is_none());
        let action = update(&mut state, Message::ShowAssistant, &data);
        assert!(matches!(action, Action::None), "no client to ask for activity: {action:?}");
        assert!(first_start(&state, &data) && selected_client(&state, &data).is_none());
        assert!(header(&state, &data, SizeClass::Wide).sub_page.is_some(), "a page of its own");

        // A starting point, then one row changed by hand: both land in the choice, not in a draft.
        let _ = update(&mut state, Message::Preset(Preset::Template(RightsTemplate::ReadOnly)), &data);
        assert_eq!(state.choice, RightsTemplate::ReadOnly.rights());
        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Tasks, Level::Write)), &data);
        assert_eq!(state.choice.area(Area::Tasks).level, Level::Write);
        assert!(subscription(&state).units() == 0, "nothing to save: there is no client yet");
        let chosen = state.choice.clone();

        let action = update(&mut state, Message::TurnOn, &data);
        let Action::Send(Request::CreateClient { name, kind, template, agent, rights }) = action else {
            panic!("expected clients.create_client");
        };
        assert_eq!(
            (name.as_str(), kind.as_str(), template, agent),
            (CLIENT_NAME, ASSISTANT_CLIENT, RightsTemplate::ReadOnly, None)
        );
        assert_eq!(rights, Some(chosen));
        assert!(matches!(update(&mut state, Message::TurnOn, &data), Action::None), "sent once");
        assert!(state.creating);
        for class in [SizeClass::Wide, SizeClass::Narrow] {
            assert_eq!(header(&state, &data, class).actions.len(), 1, "busy, still one button");
        }

        // The key of the new client is dropped; the reload then opens its detail page.
        let client = client("a1", CLIENT_NAME, ASSISTANT_CLIENT, RightsTemplate::ReadOnly.rights());
        let key = IssuedKey::new(Key::new("torro_a1_00ff"));
        let _ = answered(&mut state, &Ok(Response::ClientKey { client: client.clone(), key }));
        assert!(state.issued.is_none(), "no page ever shows the assistant's key");
        let on = Data { clients: vec![client], ..data.clone() };
        let next = loaded(&mut state, &on);
        assert!(matches!(next, Some(Action::Send(Request::ClientActivity { ref id })) if id == "a1"), "{next:?}");
        assert!(!state.creating && !first_start(&state, &on));
        drop(view(&state, &on));

        // An error ends "Turn on assistant", so the button comes back.
        let mut again = State::default();
        let _ = update(&mut again, Message::ShowAssistant, &data);
        let _ = update(&mut again, Message::TurnOn, &data);
        let _ = answered(&mut again, &Err("offline".into()));
        assert!(!again.creating);
        assert!(matches!(update(&mut again, Message::TurnOn, &data), Action::Send(Request::CreateClient { .. })));
    }

    /// Task S.21: the assistant and a bot are led like every other program – one detail page with
    /// the same rights view – but they have no key to hand out, and their last button says what it
    /// really does.
    #[test]
    fn the_assistant_and_a_bot_have_no_key_and_their_own_words() {
        let assistant = client("a1", CLIENT_NAME, ASSISTANT_CLIENT, RightsTemplate::ReadAndWrite.rights());
        let bot = client("c0000001", "Travel planner", BOT_CLIENT, Rights::none());
        let data = Data { clients: vec![assistant, bot], ..data() };
        assert_eq!(Category::of(&data.clients[0]), Category::Assistant);
        assert!(!Category::Assistant.has_key() && !Category::Bot.has_key(), "the core holds their keys");
        assert!(Category::Agent.has_key() && Category::Script.has_key() && Category::Hook.has_key());
        assert_eq!(Category::Assistant.away_label(), Some(tr("Turn off assistant…")));
        assert_eq!(Category::Assistant.confirm_word(), tr("Turn off"));
        assert_eq!(Category::Agent.away_label(), Some(tr("Disconnect…")));
        assert_eq!(access_title(Category::Assistant), tr("What the assistant may use"));
        assert_eq!(access_title(Category::Bot), tr("What the bot may use"));
        assert_eq!(access_title(Category::Agent), tr("Access"));

        // The assistant's page: its rights are saved like an agent's, and "Turn off assistant…"
        // removes the client.
        let mut state = State::default();
        let action = update(&mut state, Message::ShowAssistant, &data);
        assert!(matches!(action, Action::Send(Request::ClientActivity { ref id }) if id == "a1"), "{action:?}");
        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Calendar, Level::NoAccess)), &data);
        let (id, rights) = saved(&mut state, &data);
        assert_eq!((id.as_str(), rights.area(Area::Calendar).level), ("a1", Level::NoAccess));
        // "Create new key" is not even reachable for them, and the handler refuses it too.
        let _ = update(&mut state, Message::AskNewKey, &data);
        assert!(matches!(update(&mut state, Message::NewKey, &data), Action::None), "no key of its own");
        let _ = update(&mut state, Message::AskDisconnect, &data);
        assert!(overlay(&state, &data).is_some(), "Turn off the assistant?");
        let action = update(&mut state, Message::Disconnect, &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "a1"), "{action:?}");
        assert!(!state.assistant && state.choice == Rights::assistant(), "it starts from the suggestion again");

        // A bot's page is a client's page like any other, but it never takes the bot's access
        // away: only the next `bots.save_bot` in the assistant could give it a client again.
        let mut bot = State::default();
        let _ = update(&mut bot, Message::Open("c0000001".into()), &data);
        drop(view(&bot, &data));
        assert_eq!(Category::Bot.away_label(), None, "no destructive button on a bot's page");
        assert!(matches!(update(&mut bot, Message::AskDisconnect, &data), Action::None), "and no question either");
        assert!(overlay(&bot, &data).is_none());
        assert!(matches!(update(&mut bot, Message::Disconnect, &data), Action::None), "nor a way past it");
    }

    /// Task S.21, found in the review of S.22: "Open chat" is back on a bot's page, the way the
    /// control centre's own bot page had it – with the bot's id, not its client's. Every other
    /// detail page has no such button.
    #[test]
    fn a_bot_s_page_opens_its_chat() {
        let bot_client = client("c0000001", "Travel planner", BOT_CLIENT, Rights::none());
        let agent = client("c1", "Claude Code", "claude-code", RightsTemplate::ReadAndWrite.rights());
        let summary = torro_ipc::bots::BotSummary {
            id: "b0000001".into(),
            name: "Travel planner".into(),
            description: "Plans trips".into(),
            avatar: torro_ipc::assistant::LETTER_AVATAR.into(),
            engine: "ai:ollama".into(),
            model: None,
            client: "c0000001".into(),
        };
        let data = Data { clients: vec![bot_client, agent], bots: vec![summary], ..data() };

        let mut state = State::default();
        let _ = update(&mut state, Message::Open("c0000001".into()), &data);
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            let header = header(&state, &data, class);
            assert_eq!(header.actions.len(), 1, "{class:?}: the bot's page offers \"Open chat\"");
        }
        let action = update(&mut state, Message::OpenChat("b0000001".into()), &data);
        assert!(
            matches!(
                action,
                Action::Send(Request::OpenChatWindow { window: torro_ipc::assistant::ChatWindow::Bot(ref id) })
                    if id == "b0000001"
            ),
            "{action:?}"
        );

        // An agent's page has none, and neither has a bot the core no longer lists.
        let mut other = State::default();
        let _ = update(&mut other, Message::Open("c1".into()), &data);
        assert!(header(&other, &data, SizeClass::Wide).actions.is_empty());
        let gone = Data { bots: Vec::new(), ..data };
        assert!(header(&state, &gone, SizeClass::Wide).actions.is_empty(), "nothing to chat with");
    }

    /// Task S.21: the list always has the assistant's group – its row turns it on while it is off –
    /// and the empty state only speaks of the other programs.
    #[test]
    fn the_list_always_shows_the_assistant() {
        let now = Utc::now();
        let mut empty = Data { clients: Vec::new(), ..data() };
        let state = State::default();
        drop(list(&state, &empty, now));
        empty.clients.push(client("a1", CLIENT_NAME, ASSISTANT_CLIENT, Rights::assistant()));
        drop(list(&state, &empty, now));
        drop(list(&state, &data(), now));
        assert_eq!(Category::ALL[0], Category::Assistant, "its group comes first");
    }
}
