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
//! - **Detail:** the rights per grantable area as a segmented control that offers only the levels
//!   that make a difference there ([`levels`], read from the command catalogue), each starting in
//!   the same slot ([`segment_slot`]) so the levels line up; every other control of the page (the
//!   switches, "Create new key") starts at the same edge, so the controls form one column
//!   ([`control_column`]). Where a row's name and line would not fit next to that column (a
//!   narrow window), each segmented control stands on a line of its own under them instead and the
//!   switches go to the right edge; an admin switch of its own after
//!   every area that has admin commands ([`has_admin`], independent of the level); under "Mail"
//!   from "Write" on the row "Send on my behalf" (never, ask me each time, always; task M3.4,
//!   [`Rights::outbound`]), with a warning under it while it says "Always" ([`sends_freely`]), since
//!   mail can hold instructions to the agent; "Ask first"
//!   (never, on delete, on every change); the selections of calendars, task lists, GitHub
//!   repositories, inbox sources, mail accounts and the assistant's own tools (task WZ.7) ("All"
//!   or the switched-on ones, a calendar's
//!   colour as a dot as on the Accounts page; the suite's own entries are never offered, since the
//!   core shows them only to the person, task E1.2c) for each area the client can reach; its latest requests from
//!   the audit
//!   log (command and result, never contents); "Create new key" and "Disconnect…", each behind a
//!   confirmation that says what goes ([`disconnect_text`]). A new key is shown once, with a copy
//!   button.
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
//!   they are on ([`bot_warning`]).
//! - **Hook clients** (`api::HOOK_CLIENT`) show the agent they report for and their fixed rights,
//!   read-only: the core narrows a hook's rights to `Rights::hook` whatever is sent, because its key
//!   sits in files the agent can read itself.
//!
//! Every change edits a draft of the client's rights, which the page shows at once; [`SAVE_DELAY`]
//! after the last change (`docs/settings.md`: autosave after 500 ms) it goes out as
//! `clients.update_client` with the whole rights. The draft stays on screen until the core answered
//! and the reload after it arrived ([`answered`], [`loaded`]), so no control jumps back for a moment.
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
//! `tasks.list_lists` (whose GitHub filter lists also name repositories), `inbox.list_sources`,
//! `accounts.list_accounts` (the accounts of the kind `mail`) and `tools.list_tools`, all loaded by
//! the shell into [`Data`]. Only the app key may send `clients.*`; the control centre
//! has it.

use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use iced::time::Instant;
use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{Space, center, column, container, responsive, row, svg, text_input, toggler};
use iced::{Alignment, Color, Element, Fill, Length, Subscription, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Status, color_dot};
use torro_gui::{style, typo};
use torro_inbox::{AgentKind, SourceKind, SourceRef};
use torro_ipc::api::{
    self, ASSISTANT_CLIENT, Access, Area, AreaRights, BOT_CLIENT, Confirm, HOOK_CLIENT, Level, Rights,
};
use torro_ipc::api::{Outbound, RightsTemplate, Selection, has_command};
// Shared with the bot editor of the assistant (task B.5); `ui/bots.rs` and `ui/assistant.rs` reach
// them here as before.
pub(super) use torro_ipc::api::{active_level, levels, shows_area};
use torro_ipc::{ActivityEntry, ClientInfo, IssuedKey, Request, Response, SettingKey};
use torro_pim::Collection;

use super::accounts::relative_time;
use super::{Action, Data, Header, agent_wizard, chevron, confirm, dialog, pressable, row_frame, setting_row};
#[cfg(test)]
use crate::client_setup::Released;
use crate::client_setup::{Job, Outcome};

/// How long after the last change the rights are saved (`docs/settings.md`, "Speichern
/// automatisch").
pub const SAVE_DELAY: Duration = Duration::from_millis(500);

/// How often the page looks whether a draft is due while one waits; nothing ticks otherwise.
const TICK: Duration = Duration::from_millis(100);

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
    /// Rights changed on the page and not yet in [`Data::clients`].
    draft: Option<Draft>,
    /// The client whose activity was asked for and not answered yet.
    asked: Option<String>,
    /// The latest requests of a client, as `clients.activity` answered for it.
    activity: Option<(String, Vec<ActivityEntry>)>,
    /// The confirmation that is open.
    dialog: Option<Dialog>,
    /// A key the core just handed out, shown once until "Done".
    issued: Option<Issued>,
    /// What is typed into "Add repository".
    repo_input: String,
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
            draft: None,
            asked: None,
            activity: None,
            dialog: None,
            issued: None,
            repo_input: String::new(),
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

/// Rights the page changed, on their way to the core.
#[derive(Debug)]
struct Draft {
    id: String,
    rights: Rights,
    phase: Phase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Changed at this instant; saved [`SAVE_DELAY`] later unless changed again.
    Waiting(Instant),
    /// `clients.update_client` is on its way.
    Sending,
    /// The core took it; the draft goes once the reload shows the stored rights.
    Answered,
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

/// One of the six selections of a client's rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Calendars,
    TaskLists,
    GithubRepos,
    InboxSources,
    MailAccounts,
    /// The assistant's own tools ([`Rights::tools`], task WZ.7), by their id (`pdf`).
    Tools,
}

impl Pick {
    pub const ALL: [Pick; 6] =
        [Pick::Calendars, Pick::TaskLists, Pick::GithubRepos, Pick::InboxSources, Pick::MailAccounts, Pick::Tools];

    /// The area whose level decides whether the selection matters at all.
    pub fn area(self) -> Area {
        match self {
            Pick::Calendars => Area::Calendar,
            Pick::TaskLists => Area::Tasks,
            Pick::GithubRepos => Area::Github,
            Pick::InboxSources => Area::Inbox,
            Pick::MailAccounts => Area::Mail,
            Pick::Tools => Area::Tools,
        }
    }

    pub fn of(self, rights: &Rights) -> &Selection {
        match self {
            Pick::Calendars => &rights.calendars,
            Pick::TaskLists => &rights.task_lists,
            Pick::GithubRepos => &rights.github_repos,
            Pick::InboxSources => &rights.inbox_sources,
            Pick::MailAccounts => &rights.mail_accounts,
            Pick::Tools => &rights.tools,
        }
    }

    fn of_mut(self, rights: &mut Rights) -> &mut Selection {
        match self {
            Pick::Calendars => &mut rights.calendars,
            Pick::TaskLists => &mut rights.task_lists,
            Pick::GithubRepos => &mut rights.github_repos,
            Pick::InboxSources => &mut rights.inbox_sources,
            Pick::MailAccounts => &mut rights.mail_accounts,
            Pick::Tools => &mut rights.tools,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Pick::Calendars => tr("Calendars"),
            Pick::TaskLists => tr("Task lists"),
            Pick::GithubRepos => tr("GitHub repositories"),
            Pick::InboxSources => tr("Notification Hub sources"),
            Pick::MailAccounts => tr("Mail accounts"),
            Pick::Tools => tr("Tools"),
        }
    }

    /// The sentence under the group: what "All" includes, or what happens to the rest.
    fn footer(self, all: bool) -> &'static str {
        match (self, all) {
            (Pick::Calendars, true) => tr("Includes calendars added later."),
            (Pick::Calendars, false) => tr("Calendars left off don't exist for this agent."),
            (Pick::TaskLists, true) => tr("Includes lists added later, GitHub filter lists too."),
            (Pick::TaskLists, false) => tr("Lists left off don't exist for this agent."),
            (Pick::GithubRepos, true) => tr("Every repository your GitHub token reaches, private ones too."),
            (Pick::GithubRepos, false) => tr(
                "Matches from other repositories are dropped before the agent sees them. An owner stands for all of its repositories.",
            ),
            (Pick::InboxSources, true) => {
                tr("Includes sources added later. Approvals and account problems are shown only to you.")
            }
            (Pick::InboxSources, false) => tr(
                "Entries of other sources don't exist for this agent. Approvals and account problems are shown only to you.",
            ),
            (Pick::MailAccounts, true) => tr("Includes mail accounts added later."),
            (Pick::MailAccounts, false) => tr("Mail accounts left off don't exist for this agent."),
            // The switch is the person's, in the assistant's settings: the rights alone never make a
            // tool work (`docs/assistent.md`, "Schalter und Rechte sind zwei Dinge").
            (Pick::Tools, true) => {
                tr("Includes tools added later. Each works only while it is on in the assistant's settings.")
            }
            (Pick::Tools, false) => {
                tr("This agent can't use tools left off. Each works only while it is on in the assistant's settings.")
            }
        }
    }
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
    /// A segment of an area's level.
    Level(Area, Level),
    /// The admin switch of an area.
    Admin(Area, bool),
    /// A segment of "Ask first".
    Confirm(Confirm),
    /// A segment of "Send on my behalf".
    Outbound(Outbound),
    /// "Mail to cloud models" under Mail of the assistant and its bots (task C.17): the data rule,
    /// a setting of the suite (`settings.set_setting mail_to_cloud`), not a right.
    MailToCloud(bool),
    /// "All" (`true`) or "Selected" of a selection.
    All(Pick, bool),
    /// A switch of a selection: the id and whether it is on now.
    Tick(Pick, String, bool),
    /// The text of "Add repository" changed.
    RepoInput(String),
    /// "Add" next to it, or Enter in it.
    AddRepo,
    /// The clock while a draft waits ([`subscription`]).
    Clock(Instant),
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
            save_now(state)
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
                    state.repo_input.clear();
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
        // The app key's: only a person lets mail out (task C.17, `Request::kept_for_people`).
        Message::MailToCloud(on) => {
            Action::Send(Request::SetSetting { key: SettingKey::MailToCloud, value: serde_json::Value::Bool(on) })
        }
        Message::RepoInput(text) => {
            state.repo_input = text;
            Action::None
        }
        message @ (Message::Level(..)
        | Message::Admin(..)
        | Message::Confirm(_)
        | Message::Outbound(_)
        | Message::All(..)
        | Message::Tick(..)
        | Message::AddRepo) => edit(state, data, message),
        Message::Clock(now) => match &state.draft {
            Some(Draft { phase: Phase::Waiting(since), .. }) if now.saturating_duration_since(*since) >= SAVE_DELAY => {
                save_now(state)
            }
            _ => Action::None,
        },
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
            if state.draft.as_ref().is_some_and(|draft| draft.id == id) {
                state.draft = None;
            }
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
    state.repo_input.clear();
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

/// Applies the rights control `message` to the rights of the client on the detail page
/// ([`change_rights`]) and waits [`SAVE_DELAY`] before saving. On the assistant's first start there
/// is no client yet: the change goes to the choice "Turn on assistant" will send. A hook client's
/// rights are fixed: nothing changes.
fn edit(state: &mut State, data: &Data, message: Message) -> Action {
    if first_start(state, data) {
        let mut choice = state.choice.clone();
        if change_rights(&mut choice, ASSISTANT_CLIENT, message, data, &mut state.repo_input) {
            state.choice = choice;
        }
        return Action::None;
    }
    let Some(client) = selected_client(state, data) else { return Action::None };
    if client.kind == HOOK_CLIENT {
        return Action::None;
    }
    let mut rights = shown_rights(state, client).clone();
    let (id, kind) = (client.id.clone(), client.kind.as_str());
    if change_rights(&mut rights, kind, message, data, &mut state.repo_input) {
        state.draft = Some(Draft { id, rights, phase: Phase::Waiting(Instant::now()) });
    }
    Action::None
}

/// Applies a control of the rights view ([`rights_groups`]) to `rights` of a client of `kind`: an
/// area's level or admin switch, "Ask first", "Send on my behalf", "All"/"Selected" or a switch of a
/// selection, "Add" of a repository (`repo_input` is the text typed there; it is cleared once the
/// name is added). The agents' detail page and the Assistant page (task C.6) share it, so both
/// views change rights the same way.
///
/// Returns `true` when `rights` changed. Every other message, a control that changes nothing (the
/// level already set, "Selected" while already selected, a name that is no repository) and an area
/// the client's kind does not show ([`shows_area`]) leave `rights` as they are and return `false`.
/// "Selected" starts with every known candidate ticked ([`candidates`]): the client keeps what it
/// reaches until the person unticks something. A level goes through [`Rights::set_level`], the rule
/// the bot editor shares (task B.5): switching an area on while nothing of it is ticked ticks
/// everything of it there is now – calendars, task lists, inbox sources, signed-in mail accounts
/// (issue #133, widened), the tools `tools.list_tools` lists (task WZ.7), switched on or not –,
/// GitHub stays empty; what is added later stays off until ticked.
pub(super) fn change_rights(
    rights: &mut Rights,
    kind: &str,
    message: Message,
    data: &Data,
    repo_input: &mut String,
) -> bool {
    let before = rights.clone();
    match message {
        Message::Level(area, level) if shows_area(area, kind) => {
            let present = || match Pick::ALL.into_iter().find(|pick| pick.area() == area) {
                Some(pick) => candidates(pick, data, &Selection::Only(Vec::new())).into_iter().map(|c| c.id).collect(),
                None => Vec::new(),
            };
            rights.set_level(area, level, present);
        }
        Message::Admin(area, admin) if shows_area(area, kind) => {
            let level = rights.area(area).level;
            rights.areas.insert(area, AreaRights { level, admin });
        }
        Message::Confirm(confirm) => rights.confirm = confirm,
        Message::Outbound(outbound) => rights.outbound = outbound,
        Message::All(pick, true) => *pick.of_mut(rights) = Selection::All,
        Message::All(pick, false) if !matches!(pick.of(rights), Selection::Only(_)) => {
            let ids = candidates(pick, data, pick.of(rights)).into_iter().map(|candidate| candidate.id).collect();
            *pick.of_mut(rights) = Selection::Only(ids);
        }
        Message::Tick(pick, id, on) => tick(pick.of_mut(rights), id, on),
        Message::AddRepo => {
            let Some(repo) = valid_repo(repo_input) else { return false };
            repo_input.clear();
            tick(&mut rights.github_repos, repo, true);
        }
        _ => {}
    }
    *rights != before
}

/// The warning under an area's row of a client of `kind` with `rights`, while that area reaches the
/// bots (task B.3): `bots` lets the client act with a bot's rights, `assistant` read the
/// conversations with bots too. `None` for every other area, while the area is off, and for the
/// assistant and the bots themselves, which never get those rows ([`shows_area`]).
pub fn bot_warning(area: Area, rights: &Rights, kind: &str) -> Option<&'static str> {
    if !shows_area(area, kind) || rights.area(area).level == Level::NoAccess {
        return None;
    }
    match area {
        Area::Bots => Some(tr("Lets the agent act with a bot's rights.")),
        Area::Assistant => Some(tr("Also reads the conversations with bots.")),
        _ => None,
    }
}

/// A warning in a row of its own: a mark in the attention colour and the sentence, which wraps in a
/// narrow window.
fn warning_row<'a>(words: &'a str) -> Element<'a, Message> {
    let mark = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).attention_text) });
    let words = typo::text(typo::meta(), words).style(theme::text_2);
    let content = row![mark, words.width(Fill)].spacing(space::m());
    container(content).width(Fill).padding([space::m(), space::l()]).into()
}

/// Sends a waiting draft now: `clients.update_client` with the whole rights.
fn save_now(state: &mut State) -> Action {
    match state.draft.as_mut() {
        Some(draft) if matches!(draft.phase, Phase::Waiting(_)) => {
            draft.phase = Phase::Sending;
            Action::Send(Request::UpdateClient { id: draft.id.clone(), rights: draft.rights.clone() })
        }
        _ => Action::None,
    }
}

/// Ticks (`on`) or unticks `id` in `selection`. In "All" nothing is ticked off, so nothing changes.
fn tick(selection: &mut Selection, id: String, on: bool) {
    let Selection::Only(ids) = selection else { return };
    let present = ids.contains(&id);
    if on && !present {
        ids.push(id);
    } else if !on {
        ids.retain(|selected| *selected != id);
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
    let for_draft = state.draft.as_ref().is_some_and(|draft| draft.phase == Phase::Sending);
    if for_draft {
        match result {
            Ok(Response::Ok) => {
                if let Some(draft) = state.draft.as_mut() {
                    draft.phase = Phase::Answered;
                }
            }
            Err(_) => state.draft = None,
            Ok(_) => {}
        }
    }
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
    if state.draft.as_ref().is_some_and(|draft| draft.phase == Phase::Answered) {
        state.draft = None;
    }
    let client = assistant_client(data)?;
    state.creating = false;
    if !state.assistant || state.selected.as_deref() == Some(client.id.as_str()) {
        return None;
    }
    Some(open(state, client.id.clone()))
}

/// The clock that saves a waiting draft; none while nothing waits, so the page costs nothing at
/// rest.
pub fn subscription(state: &State) -> Subscription<Message> {
    match state.draft {
        Some(Draft { phase: Phase::Waiting(_), .. }) => iced::time::every(TICK).map(Message::Clock),
        _ => Subscription::none(),
    }
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

/// The rights the page shows for `client`: its draft while there is one, else what the core holds.
pub(super) fn shown_rights<'a>(state: &'a State, client: &'a ClientInfo) -> &'a Rights {
    match &state.draft {
        Some(draft) if draft.id == client.id => &draft.rights,
        _ => &client.rights,
    }
}

/// Whether `area` has admin commands agents may be allowed, so its admin switch is shown.
pub fn has_admin(area: Area) -> bool {
    has_command(area, Access::Admin)
}

/// The options of "Send on my behalf", least allowed first.
fn outbound_options() -> [(&'static str, Outbound); 3] {
    [(tr("Never"), Outbound::Never), (tr("Ask me each time"), Outbound::WithApproval), (tr("Always"), Outbound::Free)]
}

/// Whether the row "Send on my behalf" is shown for `level` of the mail area: sending is a write
/// (plus the switch of its own), so below "Write" the core refuses it whatever the switch says.
pub fn can_send(level: Level) -> bool {
    level >= Level::Write
}

/// Whether the warning under "Send on my behalf" is due: the client sends without asking. Mail it
/// reads can hold instructions to it, and with "Always" nobody looks before it sends.
pub fn sends_freely(rights: &Rights) -> bool {
    can_send(rights.area(Area::Mail).level) && rights.outbound == Outbound::Free
}

/// The width of the slot every segmented control of the detail page starts in, left-aligned: as
/// wide as the widest of them (all four levels, "Ask first", "Send on my behalf", "All · Selected")
/// in the language shown. So "None" stands under "None" whether an area offers two levels or four,
/// and the page lines up in every size class without a width of its own.
pub fn segment_slot() -> f32 {
    let levels = [Level::NoAccess, Level::Read, Level::Write, Level::Full].map(widgets::level_label);
    let confirm = widgets::confirm_options().map(|(label, _)| label);
    let outbound = outbound_options().map(|(label, _)| label);
    let pick = [tr("All"), tr("Selected")];
    [&levels[..], &confirm, &outbound, &pick].into_iter().map(widgets::segmented_width).fold(0.0, f32::max)
}

/// Where the controls of the detail page go on a page `width` wide: `Some(slot)`, a column
/// [`segment_slot`] wide at the right of every row, when the longest name and line of an area or
/// admin switch fit next to it in a `setting_row`; `None` when they would be cut off (a narrow
/// window, the narrow end of the medium one), then every segmented control stands on a line of its
/// own under its name (`widgets::segment_row`) and the other controls go to the right edge.
/// Decided by what fits (`widgets::segment_column`) rather than by the size class, which the page
/// does not know; it also holds for longer translations.
pub fn control_column(width: f32) -> Option<f32> {
    let admins: Vec<(&str, String)> = Area::ALL
        .into_iter()
        .filter(|&area| area.grantable() && has_admin(area))
        .map(|area| (admin_label(area), admin_detail(area)))
        .collect();
    let areas =
        Area::ALL.into_iter().filter(|area| area.grantable()).map(|area| (area_label(area), Some(area_detail(area))));
    let admins = admins.iter().map(|(label, detail)| (*label, Some(detail.as_str())));
    let sending = (tr("Send on my behalf"), Some(outbound_detail()));
    widgets::segment_column(segment_slot(), areas.chain(admins).chain([sending]), width)
}

/// `control` where the page puts its controls: at the left of the column, or as it is (at the
/// right edge of its row) without one ([`control_column`]).
pub(super) fn in_column<'a, M: 'a>(control: impl Into<Element<'a, M>>, column: Option<f32>) -> Element<'a, M> {
    match column {
        Some(slot) => container(control).width(slot).into(),
        None => control.into(),
    }
}

/// The area's name for people, translated: `Calendar`, `Agents & API`. The assistant's tool lines
/// name areas with it too (issue #133).
pub(crate) fn area_label(area: Area) -> &'static str {
    match area {
        Area::System => tr("System"),
        Area::Workspaces => tr("Workspaces"),
        Area::Calendar => tr("Calendar"),
        Area::Tasks => tr("Tasks"),
        Area::Github => tr("GitHub"),
        Area::Inbox => tr("Notification Hub"),
        Area::Mail => tr("Mail"),
        Area::Accounts => tr("Accounts"),
        Area::Ai => tr("AI"),
        Area::Assistant => tr("Assistant"),
        Area::Bots => tr("Bots"),
        Area::Tools => tr("Tools"),
        Area::Settings => tr("Settings"),
        Area::Apps => tr("Apps"),
        Area::Clients => tr("Agents & API"),
        Area::Approvals => tr("Approvals"),
    }
}

/// What an area holds, under its name.
fn area_detail(area: Area) -> &'static str {
    match area {
        Area::Workspaces => tr("Templates of your windows"),
        Area::Calendar => tr("Calendars and events"),
        Area::Tasks => tr("Task lists and tasks"),
        Area::Github => tr("Issues and pull requests in filter lists"),
        Area::Inbox => tr("Entries of agents and scripts"),
        Area::Mail => tr("Folders, messages and drafts"),
        Area::Accounts => tr("Connected accounts and sync"),
        Area::Ai => tr("Models of your AI accounts"),
        Area::Assistant => tr("Chats with the assistant, which acts with its own rights"),
        Area::Bots => tr("Bots and asking them, which act with their own rights"),
        Area::Tools => tr("The assistant's own tools, such as making PDFs"),
        Area::Settings => tr("Settings of the suite"),
        Area::Apps => tr("Opening the suite's apps"),
        Area::System | Area::Clients | Area::Approvals => "",
    }
}

/// What the admin switch of an area allows.
fn admin_label(area: Area) -> &'static str {
    match area {
        Area::Accounts => tr("Connect and remove accounts"),
        Area::Settings => tr("Change settings"),
        Area::Inbox => tr("Mute sources and keep rules"),
        _ => tr("Admin commands"),
    }
}

/// The line under an admin switch: the area it belongs to and that every use waits for approval
/// ("Accounts · each time with your approval").
pub fn admin_detail(area: Area) -> String {
    trf("{area} · each time with your approval", &[("area", &area_label(area))])
}

pub(super) fn template_label(template: RightsTemplate) -> &'static str {
    match template {
        RightsTemplate::ReadOnly => tr("Read only"),
        RightsTemplate::ReadAndWrite => tr("Read and write"),
        RightsTemplate::FullAccess => tr("Full access"),
    }
}

/// A client's rights in a few words for the list: the template they equal ("Read and write"),
/// else "Custom: Calendar, Tasks, Inbox" (at most three areas, then "+2"); "No access" without
/// any area; for a hook what it can do at all.
pub fn rights_summary(rights: &Rights, kind: &str) -> String {
    if kind == HOOK_CLIENT {
        return tr("Reports to the Notification Hub").to_string();
    }
    if let Some(template) = rights.template() {
        return template_label(template).to_string();
    }
    let areas: Vec<&str> = Area::ALL
        .into_iter()
        .filter(|&area| area.grantable() && rights.area(area).level != Level::NoAccess)
        .map(area_label)
        .collect();
    let shown = areas[..areas.len().min(3)].join(", ");
    match areas.len() {
        0 => tr("No access").to_string(),
        1..=3 => trf("Custom: {areas}", &[("areas", &shown)]),
        n => trf("Custom: {areas} +{n}", &[("areas", &shown), ("n", &(n - 3))]),
    }
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

/// A row of a selection: what is switched on by `id`, its name, a line below it and its colour
/// (for calendars and lists, shown as a dot before the name).
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub color: Option<Color>,
}

impl Candidate {
    fn plain(id: String, label: String, detail: Option<String>) -> Candidate {
        Candidate { id, label, detail, color: None }
    }
}

/// What a selection offers: everything the core knows of its kind, then what `selection` names
/// that the core no longer knows (marked, so it can be unticked).
pub fn candidates(pick: Pick, data: &Data, selection: &Selection) -> Vec<Candidate> {
    let mut known = match pick {
        Pick::Calendars => data.calendars.iter().map(|collection| collection_candidate(collection, data)).collect(),
        Pick::TaskLists => data.task_lists.iter().map(|collection| collection_candidate(collection, data)).collect(),
        Pick::GithubRepos => repo_candidates(data, selection),
        Pick::InboxSources => source_candidates(data, selection),
        Pick::MailAccounts => mail_candidates(data),
        Pick::Tools => tool_candidates(data),
    };
    // Repositories need no such pass: their candidates are made from the selection.
    if let (Selection::Only(ids), false) = (selection, pick == Pick::GithubRepos) {
        let gone: Vec<Candidate> = ids
            .iter()
            .filter(|id| !known.iter().any(|candidate: &Candidate| candidate.id == **id))
            .map(|id| Candidate::plain(id.clone(), id.clone(), Some(tr("No longer available").to_string())))
            .collect();
        known.extend(gone);
    }
    known
}

/// A calendar or list: its colour, and the account it belongs to (a GitHub filter list says so).
fn collection_candidate(collection: &Collection, data: &Data) -> Candidate {
    let account = data
        .accounts
        .iter()
        .find(|account| account.collections.iter().any(|known| known.id == collection.id))
        .map(|account| account.label.clone());
    let detail = match collection.source {
        Some(_) => Some(tr("GitHub filter list").to_string()),
        None => account,
    };
    Candidate {
        id: collection.id.clone(),
        label: collection.name.clone(),
        detail,
        color: Some(widgets::collection_color(collection.color.as_deref())),
    }
}

/// The repositories and owners the GitHub filter lists search (`repo:owner/name`, `org:owner`,
/// `user:owner`) and those `selection` names, each once whatever its case (as the selection
/// spells it), in alphabetical order. There is no list of all repositories; others are typed in ("Add repository").
fn repo_candidates(data: &Data, selection: &Selection) -> Vec<Candidate> {
    // The selection's spelling first, so its names show ticked.
    let mut names: Vec<String> = match selection {
        Selection::Only(selected) => selected.clone(),
        Selection::All => Vec::new(),
    };
    for name in repos_in(&data.task_lists) {
        if !names.iter().any(|known| known.eq_ignore_ascii_case(&name)) {
            names.push(name);
        }
    }
    names.sort_by_key(|name| name.to_lowercase());
    names
        .into_iter()
        .map(|name| {
            let detail = (!name.contains('/')).then(|| tr("All repositories of this owner").to_string());
            Candidate::plain(name.clone(), name, detail)
        })
        .collect()
}

/// The repositories and owners named in the searches of the GitHub filter lists among
/// `collections`, each once whatever its case. Excluding ones (`-repo:`) are not offered.
pub fn repos_in(collections: &[Collection]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let queries = collections.iter().filter_map(|collection| collection.source.as_ref()).flat_map(|f| &f.queries);
    for word in queries.flat_map(|query| query.split_whitespace()) {
        let name = ["repo:", "org:", "user:"].iter().find_map(|prefix| word.strip_prefix(prefix));
        let Some(name) = name.map(|name| name.trim_matches('"')).filter(|name| !name.is_empty()) else { continue };
        if !names.iter().any(|known| known.eq_ignore_ascii_case(name)) {
            names.push(name.to_string());
        }
    }
    names
}

/// `owner/name` or `owner` as typed into "Add repository", trimmed; `None` for anything else
/// (spaces, more than one `/`, an empty part, characters GitHub does not allow in names).
pub fn valid_repo(input: &str) -> Option<String> {
    let input = input.trim();
    let part = |part: &str| {
        !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    let valid = match input.split_once('/') {
        Some((owner, name)) => part(owner) && part(name),
        None => part(input),
    };
    valid.then(|| input.to_string())
}

/// The inbox sources an agent can be allowed: the sources with entries or muted
/// (`inbox.list_sources`), the source of every connected client (where its reports would land,
/// [`api::post_source`]), every mail account (its new mail, task E4.1: the source's key is the
/// account's id), the inbox sources of every GitHub, Nextcloud and ntfy account (task E5.14:
/// GitHub, Nextcloud, Talk, ntfy, [`remote_sources`]) and those `selection` names; each once, in the
/// order of [`SourceKind`] (agents, scripts, mail, GitHub, Nextcloud, Talk, ntfy). Each remote source
/// says its account below its name; GitHub adds that its entries also need read access to GitHub
/// and the repository, which the core checks besides the source (task E5.2).
/// Never the suite: its entries (approvals, account problems) are only for the person, the core
/// shows them to no client whatever the selection says (task E1.2c). A selection that still names
/// it shows it as no longer available ([`candidates`]), so it can be unticked.
fn source_candidates(data: &Data, selection: &Selection) -> Vec<Candidate> {
    let mut sources = api::present_sources(&data.sources, &data.clients, &data.accounts);
    if let Selection::Only(keys) = selection {
        sources.extend(keys.iter().filter_map(|key| key.parse::<SourceRef>().ok()));
    }
    sources.retain(|source| source.kind != SourceKind::Suite);
    sources.sort();
    sources.dedup();
    sources
        .into_iter()
        .map(|source| {
            let detail = match source.kind {
                SourceKind::Agent => tr("Agent").to_string(),
                SourceKind::Script | SourceKind::Suite => tr("Script").to_string(),
                SourceKind::Mail => tr("Mail").to_string(),
                SourceKind::Github => {
                    trf("{account} · Also needs read access to GitHub and the repository", &[("account", &source.name)])
                }
                SourceKind::Nextcloud | SourceKind::Talk | SourceKind::Ntfy => source.name.clone(),
            };
            Candidate::plain(source.key(), source.label(), Some(detail))
        })
        .collect()
}

/// The mail accounts an agent can be allowed (`accounts.list_accounts`, kind `mail`): the id is
/// what the core checks a call against ([`Rights::reaches_mail_account`]), the address is what the
/// person knows it by. A sign-in that is still running has no id and no mail yet.
fn mail_candidates(data: &Data) -> Vec<Candidate> {
    api::signed_in_mail(&data.accounts)
        .map(|account| Candidate::plain(account.id.clone(), account.label.clone(), None))
        .collect()
}

/// The assistant's own tools (`tools.list_tools`, task WZ.7): the id is what the core checks a
/// command against ([`Rights::reaches_tool`]), the name what people call it ("PDF"). Below it what
/// the tool does, or, while it is off in the assistant's settings, that it is: granted, it still
/// does nothing until the person switches it on there.
fn tool_candidates(data: &Data) -> Vec<Candidate> {
    data.tools
        .iter()
        .map(|tool| {
            let detail = match tool.enabled {
                true => tool.description.clone(),
                false => tr("Off in the assistant's settings").to_string(),
            };
            Candidate::plain(tool.id.name().to_string(), tool.name.clone(), Some(detail))
        })
        .collect()
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

/// The detail page in the width the page has ([`control_column`]).
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
        page =
            page.extend(rights_groups(&state.repo_input, rights, data, column, &client.kind, access_title(category)));
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
    let groups = rights_groups(
        &state.repo_input,
        &state.choice,
        data,
        column,
        ASSISTANT_CLIENT,
        access_title(Category::Assistant),
    );
    column![heading(CLIENT_NAME, about), off_group(), preset_group(&state.choice)]
        .extend(groups)
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

/// The rights view of a client of `kind` (task A.6, shared with the Assistant page of task C.6): the
/// access group titled `title` ([`access_group`]), "Ask first", and the selection of each area the
/// rights reach (calendars, task lists, repositories, inbox sources, mail accounts, tools).
/// `repo_input` is the text in "Add repository". Every control sends a message that
/// [`change_rights`] applies; the caller keeps the rights and saves them.
pub(super) fn rights_groups<'a>(
    repo_input: &'a str,
    rights: &Rights,
    data: &'a Data,
    column: Option<f32>,
    kind: &str,
    title: &'a str,
) -> Vec<Element<'a, Message>> {
    let mut groups = vec![
        access_group(rights, column, kind, title, data.settings.mail_to_cloud),
        confirm_group(rights.confirm, column),
    ];
    for pick in Pick::ALL.into_iter().filter(|pick| rights.area(pick.area()).level != Level::NoAccess) {
        groups.push(selection_group(repo_input, pick, rights, data, column));
    }
    groups
}

/// The level of every area a client of `kind` can be given ([`shows_area`]), each followed by its
/// admin switch where it has one. The switch is a row of its own, not indented: admin commands are
/// allowed per area whatever the level (`docs/api.md`, "Rechte"), so its line names the area
/// instead. Under Mail of the assistant and its bots, the data rule ([`data_rule_row`]): it decides
/// whether cloud models get the mail the rights grant (`mail_to_cloud`).
fn access_group<'a>(
    rights: &Rights,
    column: Option<f32>,
    kind: &str,
    title: &'a str,
    mail_to_cloud: bool,
) -> Element<'a, Message> {
    let mut rows = Vec::new();
    for area in Area::ALL.into_iter().filter(|&area| shows_area(area, kind)) {
        let granted = rights.area(area);
        let offered = levels(area);
        let active = active_level(&offered, granted.level);
        let control = widgets::segmented(
            offered.into_iter().map(|level| (widgets::level_label(level), Message::Level(area, level))),
            active,
        );
        rows.push(widgets::segment_row(area_label(area), Some(area_detail(area).to_string()), control, column));
        rows.extend(bot_warning(area, rights, kind).map(warning_row));
        if area == Area::Mail && can_send(granted.level) {
            rows.extend(outbound_rows(rights, column));
        }
        if area == Area::Mail && granted.level != Level::NoAccess && shows_data_rule(kind) {
            rows.push(data_rule_row(mail_to_cloud, column));
        }
        if has_admin(area) {
            let switch = toggler(granted.admin)
                .size(style::CHECK_SIZE)
                .style(style::toggler)
                .on_toggle(move |admin| Message::Admin(area, admin));
            rows.push(setting_row(None, admin_label(area), Some(admin_detail(area)), in_column(switch, column)));
        }
    }
    let footer = tr("Write includes reading, Full also deletes and overwrites.");
    widgets::group(title, rows, Some(footer.to_string()))
}

/// Whether the rights of a client of `kind` meet the data rule (task C.17): the assistant's and its
/// bots', which may answer with a cloud model. Other agents read through their own key whatever
/// their rights grant.
fn shows_data_rule(kind: &str) -> bool {
    kind == ASSISTANT_CLIENT || kind == BOT_CLIENT
}

/// "Mail to cloud models" (task C.17): off, only AI accounts on the person's computers read the
/// mail granted above; on, models elsewhere too. Named after what it still does: coding agents are
/// not offered as engines any more (task C.18, issue #219), what it blocks has not changed.
fn data_rule_row<'a>(mail_to_cloud: bool, column: Option<f32>) -> Element<'a, Message> {
    let switch = toggler(mail_to_cloud).size(style::CHECK_SIZE).style(style::toggler).on_toggle(Message::MailToCloud);
    let detail = match mail_to_cloud {
        false => tr("Off: only AI accounts on your own computers read mail"),
        true => tr("Mail goes to the model's provider and can't be taken back"),
    };
    setting_row(None, tr("Mail to cloud models"), Some(detail.to_string()), in_column(switch, column))
}

/// The line under "Send on my behalf": what sending means, so "Never" reads as a real choice.
fn outbound_detail() -> &'static str {
    tr("Messages leave your computer and can't be taken back")
}

/// "Send on my behalf" under the mail area, and while it says "Always" the warning in a row of its
/// own: a mark in the attention colour and the sentence, which wraps in a narrow window.
fn outbound_rows<'a>(rights: &Rights, column: Option<f32>) -> Vec<Element<'a, Message>> {
    let options = outbound_options();
    let active = options.iter().position(|(_, outbound)| *outbound == rights.outbound).unwrap_or(1);
    let control = widgets::segmented(options.map(|(label, outbound)| (label, Message::Outbound(outbound))), active);
    let mut rows =
        vec![widgets::segment_row(tr("Send on my behalf"), Some(outbound_detail().to_string()), control, column)];
    if sends_freely(rights) {
        rows.push(warning_row(tr(
            "Mail can contain instructions aimed at agents. With Always, this agent sends without asking you first.",
        )));
    }
    rows
}

/// "Ask first": never, on delete, on every change.
fn confirm_group<'a>(current: Confirm, column: Option<f32>) -> Element<'a, Message> {
    let options = widgets::confirm_options();
    let active = options.iter().position(|(_, confirm)| *confirm == current).unwrap_or(0);
    let control = widgets::segmented(options.map(|(label, confirm)| (label, Message::Confirm(confirm))), active);
    let footer = tr("The agent waits until you allow it in Approvals. Admin commands always wait.");
    let row = widgets::segment_row(tr("Ask first"), None, control, column);
    widgets::group(tr("Approval"), vec![row], Some(footer.to_string()))
}

/// A selection: "All" or "Selected", and with "Selected" a switch per candidate; for GitHub
/// also a field to add a repository or owner.
fn selection_group<'a>(
    repo_input: &'a str,
    pick: Pick,
    rights: &Rights,
    data: &'a Data,
    column: Option<f32>,
) -> Element<'a, Message> {
    let selection = pick.of(rights);
    let all = matches!(selection, Selection::All);
    let switch = widgets::segmented(
        [(tr("All"), Message::All(pick, true)), (tr("Selected"), Message::All(pick, false))],
        usize::from(!all),
    );
    let mut rows = vec![widgets::segment_row(tr("Access to"), None, switch, column)];
    if !all {
        let offered = candidates(pick, data, selection);
        if offered.is_empty() && pick != Pick::GithubRepos {
            rows.push(row_frame(
                typo::text(typo::body(), tr("Nothing to choose from yet.")).style(theme::text_3),
                false,
            ));
        }
        rows.extend(offered.into_iter().map(|candidate| {
            let ticked = selection.contains(&candidate.id);
            candidate_row(pick, candidate, ticked, column)
        }));
        if pick == Pick::GithubRepos {
            rows.push(add_repo_row(repo_input));
        }
    }
    widgets::group(pick.title(), rows, Some(pick.footer(all).to_string()))
}

/// A candidate as on the Accounts page: a calendar's or list's colour only as a dot before the
/// name (a calendar the core no longer knows keeps the dot's room, so the names line up), a
/// neutral switch at the right; the whole row toggles it.
fn candidate_row<'a>(pick: Pick, candidate: Candidate, ticked: bool, column: Option<f32>) -> Element<'a, Message> {
    let Candidate { id, label, detail, color } = candidate;
    let switch_id = id.clone();
    let switch = toggler(ticked)
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle(move |on| Message::Tick(pick, switch_id.clone(), on));
    let dot = match (pick, color) {
        (_, Some(color)) => Some(color_dot(color, space::s())),
        (Pick::Calendars | Pick::TaskLists, None) => Some(Space::new().width(space::s()).into()),
        (Pick::GithubRepos | Pick::InboxSources | Pick::MailAccounts | Pick::Tools, None) => None,
    };
    let row = setting_row(dot, label, detail, in_column(switch, column));
    pressable(row, Some(Message::Tick(pick, id, !ticked)))
}

/// "owner/name or owner" and "Add"; Enter adds too. Disabled until the text is a valid name.
fn add_repo_row(input: &str) -> Element<'_, Message> {
    let add = valid_repo(input).map(|_| Message::AddRepo);
    let field = text_input(tr("owner/name or owner"), input)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        .on_input(Message::RepoInput)
        .on_submit_maybe(add.clone());
    let button = widgets::text_button(Some(Icon::Plus), tr("Add"), style::button::secondary, add);
    row_frame(row![field, button].spacing(space::s()).align_y(Alignment::Center), false)
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
    use std::collections::BTreeMap;

    use chrono::Duration as Age;
    use torro_inbox::SourceInfo;
    use torro_ipc::tools::{ToolId, ToolInfo};
    use torro_ipc::{AccountInfo, Key};
    use torro_pim::model::GitHubFilter;
    use torro_pim::{AccountState, CollectionKind};

    use super::*;

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
            tools: vec![pdf(false)],
            ..Data::default()
        }
    }

    /// The tool PDF as `tools.list_tools` lists it for the app key, switched on or not.
    fn pdf(enabled: bool) -> ToolInfo {
        ToolInfo { id: ToolId::Pdf, name: "PDF".into(), description: "Makes PDFs.".into(), enabled, allowed: true }
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
        let later = Instant::now() + SAVE_DELAY + TICK;
        match update(state, Message::Clock(later), data) {
            Action::Send(Request::UpdateClient { id, rights }) => (id, rights),
            other => panic!("expected clients.update_client, got {other:?}"),
        }
    }

    #[test]
    fn a_change_is_saved_once_after_the_delay_with_the_whole_rights() {
        let data = data();
        let mut state = opened("c1", &data);
        assert!(matches!(update(&mut state, Message::Level(Area::Calendar, Level::Read), &data), Action::None));
        assert!(matches!(update(&mut state, Message::Confirm(Confirm::Never), &data), Action::None));
        assert!(
            matches!(update(&mut state, Message::Clock(Instant::now()), &data), Action::None),
            "nothing before the delay"
        );
        let (id, rights) = saved(&mut state, &data);
        assert_eq!(id, "c1");
        assert_eq!(rights.area(Area::Calendar), AreaRights { level: Level::Read, admin: false });
        assert_eq!(rights.confirm, Confirm::Never, "both changes in one request");
        assert_eq!(rights.area(Area::Tasks).level, Level::Full, "the rest unchanged");
        let later = Instant::now() + SAVE_DELAY + TICK;
        assert!(matches!(update(&mut state, Message::Clock(later), &data), Action::None), "sent once");
    }

    /// What the saved rights must show after a row of messages.
    type Check = fn(&Rights) -> bool;

    #[test]
    fn every_control_ends_in_update_client() {
        let data = data();
        let cases: Vec<(Vec<Message>, Check)> = vec![
            (vec![Message::Admin(Area::Accounts, true)], |r| r.area(Area::Accounts).admin),
            (vec![Message::Level(Area::Github, Level::NoAccess)], |r| r.area(Area::Github).level == Level::NoAccess),
            (vec![Message::Confirm(Confirm::OnEveryChange)], |r| r.confirm == Confirm::OnEveryChange),
            (vec![Message::All(Pick::Calendars, false)], |r| {
                r.calendars == Selection::Only(vec!["acc/work".into(), "acc/home".into()])
            }),
            (
                vec![Message::All(Pick::Calendars, false), Message::Tick(Pick::Calendars, "acc/home".into(), false)],
                |r| r.calendars == Selection::Only(vec!["acc/work".into()]),
            ),
            (vec![Message::All(Pick::TaskLists, false), Message::All(Pick::TaskLists, true)], |r| {
                r.task_lists == Selection::All
            }),
            (
                vec![
                    Message::All(Pick::GithubRepos, false),
                    Message::Tick(Pick::GithubRepos, "acme".into(), false),
                    Message::RepoInput(" anna/notes ".into()),
                    Message::AddRepo,
                ],
                |r| r.github_repos == Selection::Only(vec!["anna/torro".into(), "anna/notes".into()]),
            ),
            (vec![Message::Outbound(Outbound::Free)], |r| r.outbound == Outbound::Free),
            (vec![Message::Outbound(Outbound::Never)], |r| r.outbound == Outbound::Never),
            (vec![Message::Level(Area::Mail, Level::Read)], |r| r.area(Area::Mail).level == Level::Read),
            (vec![Message::All(Pick::MailAccounts, false)], |r| {
                r.mail_accounts
                    == Selection::Only(vec!["mail:anna@mailbox.org".into(), "mail:anna@work.example.net".into()])
            }),
            (
                vec![
                    Message::All(Pick::MailAccounts, false),
                    Message::Tick(Pick::MailAccounts, "mail:anna@work.example.net".into(), false),
                ],
                |r| r.mail_accounts == Selection::Only(vec!["mail:anna@mailbox.org".into()]),
            ),
            (vec![Message::All(Pick::MailAccounts, false), Message::All(Pick::MailAccounts, true)], |r| {
                r.mail_accounts == Selection::All
            }),
            // Issue #133: mail switched on without an account ticks the accounts signed in now…
            (
                vec![
                    Message::Level(Area::Mail, Level::NoAccess),
                    Message::All(Pick::MailAccounts, false),
                    Message::Tick(Pick::MailAccounts, "mail:anna@mailbox.org".into(), false),
                    Message::Tick(Pick::MailAccounts, "mail:anna@work.example.net".into(), false),
                    Message::Level(Area::Mail, Level::Read),
                ],
                |r| {
                    r.mail_accounts
                        == Selection::Only(vec!["mail:anna@mailbox.org".into(), "mail:anna@work.example.net".into()])
                },
            ),
            // …and keeps a choice the person made.
            (
                vec![
                    Message::Level(Area::Mail, Level::NoAccess),
                    Message::All(Pick::MailAccounts, false),
                    Message::Tick(Pick::MailAccounts, "mail:anna@work.example.net".into(), false),
                    Message::Level(Area::Mail, Level::Write),
                ],
                |r| r.mail_accounts == Selection::Only(vec!["mail:anna@mailbox.org".into()]),
            ),
            (
                vec![
                    Message::All(Pick::InboxSources, false),
                    Message::Tick(Pick::InboxSources, "script:nightly".into(), false),
                ],
                |r| {
                    let Selection::Only(keys) = &r.inbox_sources else { return false };
                    !keys.contains(&"script:nightly".to_string())
                        && keys.contains(&"agent:claude-code".to_string())
                        && !keys.contains(&"suite".to_string())
                },
            ),
            // Task WZ.7: the tools are a selection like the others.
            (vec![Message::All(Pick::Tools, false)], |r| r.tools == Selection::Only(vec!["pdf".into()])),
            (vec![Message::All(Pick::Tools, false), Message::Tick(Pick::Tools, "pdf".into(), false)], |r| {
                r.tools == Selection::Only(Vec::new())
            }),
        ];
        for (messages, check) in cases {
            let mut state = opened("c1", &data);
            let text = format!("{messages:?}");
            for message in messages {
                assert!(matches!(update(&mut state, message, &data), Action::None), "{text}: waits");
            }
            let (id, rights) = saved(&mut state, &data);
            assert_eq!(id, "c1", "{text}");
            assert!(check(&rights), "{text}: {rights:?}");
        }
    }

    /// Task B.5: the rule of issue #133 holds for every selection. Switching an area on while
    /// nothing of it is ticked ticks what there is now (never "All"); GitHub stays empty, a choice
    /// stays.
    #[test]
    fn switching_an_area_on_ticks_what_there_is_now() {
        let data = data();
        let mut repo_input = String::new();
        let mut rights = Rights::none();
        let mut level = |rights: &mut Rights, area: Area, level: Level| {
            change_rights(rights, BOT_CLIENT, Message::Level(area, level), &data, &mut repo_input)
        };
        for area in [Area::Calendar, Area::Tasks, Area::Github, Area::Inbox, Area::Mail, Area::Tools] {
            assert!(level(&mut rights, area, Level::Read), "{area:?}");
        }
        assert_eq!(rights.tools, Selection::Only(vec!["pdf".into()]), "a tool that is off is ticked too");
        assert_eq!(rights.calendars, Selection::Only(vec!["acc/work".into(), "acc/home".into()]));
        assert_eq!(rights.task_lists, Selection::Only(vec!["acc/todo".into(), "github:anna/1".into()]));
        assert_eq!(rights.github_repos, Selection::Only(Vec::new()), "no list of every repository");
        assert_eq!(
            rights.mail_accounts,
            Selection::Only(vec!["mail:anna@mailbox.org".into(), "mail:anna@work.example.net".into()])
        );
        let Selection::Only(sources) = &rights.inbox_sources else { panic!("never All") };
        assert!(sources.contains(&"script:nightly".to_string()) && sources.contains(&"agent:claude-code".to_string()));
        assert!(!sources.contains(&"suite".to_string()));

        // Off and on again keeps what was ticked meanwhile.
        rights.calendars = Selection::Only(vec!["acc/home".into()]);
        assert!(level(&mut rights, Area::Calendar, Level::NoAccess));
        assert!(level(&mut rights, Area::Calendar, Level::Write));
        assert_eq!(rights.calendars, Selection::Only(vec!["acc/home".into()]));
        assert!(!level(&mut rights, Area::Bots, Level::Read), "a bot never gets the row");
    }

    /// Task C.17: the data rule stands under Mail of the assistant and its bots only, and its switch
    /// sends the setting, never a change of rights.
    #[test]
    fn the_data_rule_is_shown_for_the_assistant_and_bots_and_sends_the_setting() {
        assert!(shows_data_rule(ASSISTANT_CLIENT) && shows_data_rule(BOT_CLIENT));
        assert!(!shows_data_rule("claude-code") && !shows_data_rule(HOOK_CLIENT));
        let data = data();
        let mut state = opened("c1", &data);
        for on in [true, false] {
            let action = update(&mut state, Message::MailToCloud(on), &data);
            let Action::Send(Request::SetSetting { key, value }) = action else { panic!("a setting") };
            assert_eq!((key, value), (SettingKey::MailToCloud, serde_json::json!(on)));
        }
        assert!(state.draft.is_none(), "no change of rights");
        let mut rights = Rights::assistant();
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
        for on in [false, true] {
            let _ = access_group(&rights, None, ASSISTANT_CLIENT, "Access", on);
        }
    }

    /// Task WZ.7: a client without tools that gets the area on the page has every tool there is
    /// ticked (`Only(["pdf"])`, never "All"), and unticking it again goes out as
    /// `clients.update_client` with the whole rights.
    #[test]
    fn switching_tools_on_ticks_the_tools_and_unticking_saves_the_rights() {
        let mut data = data();
        let rights = &mut data.clients[0].rights;
        rights.areas.insert(Area::Tools, AreaRights { level: Level::NoAccess, admin: false });
        rights.tools = Selection::Only(Vec::new());
        let mut state = opened("c1", &data);
        assert!(matches!(update(&mut state, Message::Level(Area::Tools, Level::Write), &data), Action::None));
        let (id, rights) = saved(&mut state, &data);
        assert_eq!(id, "c1");
        assert_eq!(rights.area(Area::Tools).level, Level::Write);
        assert_eq!(rights.tools, Selection::Only(vec!["pdf".into()]));

        // The core took it and the reload shows it: unticking PDF is the next request.
        data.clients[0].rights = rights;
        let mut state = opened("c1", &data);
        assert!(matches!(update(&mut state, Message::Tick(Pick::Tools, "pdf".into(), false), &data), Action::None));
        let (id, rights) = saved(&mut state, &data);
        assert_eq!(id, "c1");
        assert_eq!(rights.tools, Selection::Only(Vec::new()));
        assert_eq!(rights.area(Area::Tools).level, Level::Write, "the level stays");

        // On the assistant's first start the choice gets the same, nothing is sent.
        let mut state = State::default();
        assert!(matches!(update(&mut state, Message::ShowAssistant, &data), Action::None));
        assert!(matches!(update(&mut state, Message::Level(Area::Tools, Level::Write), &data), Action::None));
        assert_eq!(state.choice.tools, Selection::Only(vec!["pdf".into()]));
    }

    /// Task WZ.7: a tool is offered by its id and name; below it what it does, or that it is off in
    /// the assistant's settings, since granted it still does nothing there.
    #[test]
    fn tools_are_offered_with_what_they_do_or_that_they_are_off() {
        let mut data = data();
        let offered = candidates(Pick::Tools, &data, &Selection::All);
        assert_eq!(offered.iter().map(|c| (c.id.as_str(), c.label.as_str())).collect::<Vec<_>>(), [("pdf", "PDF")]);
        assert_eq!(offered[0].detail.as_deref(), Some(tr("Off in the assistant's settings")));
        assert!(offered[0].color.is_none());
        data.tools = vec![pdf(true)];
        assert_eq!(candidates(Pick::Tools, &data, &Selection::All)[0].detail.as_deref(), Some("Makes PDFs."));
        // A tool the core no longer knows stays in the list to be unticked.
        let stale = Selection::Only(vec!["pdf".into(), "chart".into()]);
        let all = candidates(Pick::Tools, &data, &stale);
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].detail.as_deref(), Some(tr("No longer available")));
        assert_eq!(Pick::Tools.area(), Area::Tools);
        // Without an answer of `tools.list_tools` there is nothing to choose from.
        assert!(candidates(Pick::Tools, &Data::default(), &Selection::All).is_empty());
    }

    #[test]
    fn mail_accounts_are_the_accounts_of_kind_mail_that_are_signed_in() {
        let data = data();
        let ids = |selection: &Selection| -> Vec<String> {
            candidates(Pick::MailAccounts, &data, selection).into_iter().map(|c| c.id).collect()
        };
        assert_eq!(ids(&Selection::All), ["mail:anna@mailbox.org", "mail:anna@work.example.net"]);
        let offered = candidates(Pick::MailAccounts, &data, &Selection::All);
        assert_eq!(offered[0].label, "anna@mailbox.org", "named by the address");
        assert!(offered[0].color.is_none());
        // An account that was removed stays in the list to be unticked.
        let stale = Selection::Only(vec!["mail:gone@example.org".into()]);
        let all = candidates(Pick::MailAccounts, &data, &stale);
        assert_eq!(all.last().map(|c| c.detail.as_deref()), Some(Some(tr("No longer available"))));
        assert_eq!(Pick::MailAccounts.area(), Area::Mail);
    }

    #[test]
    fn sending_is_offered_from_write_and_warned_about_when_free() {
        assert!(!can_send(Level::NoAccess) && !can_send(Level::Read));
        assert!(can_send(Level::Write) && can_send(Level::Full));
        let mut rights = RightsTemplate::ReadAndWrite.rights();
        assert_eq!(rights.outbound, Outbound::WithApproval, "templates ask each time");
        assert!(!sends_freely(&rights));
        rights.outbound = Outbound::Free;
        assert!(sends_freely(&rights));
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
        assert!(!sends_freely(&rights), "reading only: the row and its warning are gone");
        // Never, ask each time, always: least allowed first, as the control shows them.
        let order = outbound_options().map(|(_, outbound)| outbound);
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(outbound_options().map(|(label, _)| label), [tr("Never"), tr("Ask me each time"), tr("Always")]);
    }

    #[test]
    fn the_sending_switch_is_saved_with_the_whole_rights_and_hooks_keep_theirs() {
        let data = data();
        let mut state = opened("c1", &data);
        let _ = update(&mut state, Message::Outbound(Outbound::Free), &data);
        assert_eq!(shown_rights(&state, &data.clients[0]).outbound, Outbound::Free, "shown at once");
        let (_, rights) = saved(&mut state, &data);
        assert_eq!(rights.outbound, Outbound::Free);
        assert_eq!(rights.area(Area::Mail).level, Level::Full, "the rest unchanged");
        drop(view(&state, &data));

        let mut hook = opened("h1", &data);
        let _ = update(&mut hook, Message::Outbound(Outbound::Free), &data);
        assert!(hook.draft.is_none(), "a hook never sends mail");
    }

    #[test]
    fn back_saves_at_once_and_the_draft_shows_until_the_reload() {
        let data = data();
        let mut state = opened("c1", &data);
        let _ = update(&mut state, Message::Level(Area::Apps, Level::Read), &data);
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
        let _ = update(&mut state, Message::Level(Area::Apps, Level::Read), &data);
        let _ = saved(&mut state, &data);
        answered(&mut state, &Err("forbidden".into()));
        assert_eq!(shown_rights(&state, &data.clients[0]).area(Area::Apps).level, Level::Full);
    }

    #[test]
    fn the_clock_runs_only_while_a_change_waits() {
        let data = data();
        let mut state = opened("c1", &data);
        assert_eq!(subscription(&state).units(), 0);
        let _ = update(&mut state, Message::Confirm(Confirm::Never), &data);
        assert_eq!(subscription(&state).units(), 1);
        let _ = saved(&mut state, &data);
        assert_eq!(subscription(&state).units(), 0);
    }

    #[test]
    fn hook_rights_cannot_be_changed() {
        let data = data();
        let mut state = opened("h1", &data);
        for message in [
            Message::Level(Area::Calendar, Level::Full),
            Message::Admin(Area::Accounts, true),
            Message::All(Pick::InboxSources, true),
        ] {
            let _ = update(&mut state, message, &data);
        }
        assert!(state.draft.is_none());
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
        let _ = update(&mut state, Message::Level(Area::Apps, Level::Read), &data);
        let _ = update(&mut state, Message::AskDisconnect, &data);
        assert_eq!(state.dialog, Some(Dialog::Disconnect));
        assert!(overlay(&state, &data).is_some());
        let action = update(&mut state, Message::Disconnect, &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "c1"));
        assert!(state.selected.is_none() && state.draft.is_none(), "back to the list, nothing left to save");

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
    fn levels_offer_only_what_makes_a_difference() {
        let all = vec![Level::NoAccess, Level::Read, Level::Write, Level::Full];
        assert_eq!(levels(Area::Calendar), all);
        assert_eq!(levels(Area::Workspaces), all);
        assert_eq!(levels(Area::Settings), vec![Level::NoAccess, Level::Read], "its changes are admin commands");
        assert!(has_admin(Area::Accounts) && has_admin(Area::Settings));
        assert!(!has_admin(Area::Calendar));
        assert_eq!(active_level(&[Level::NoAccess, Level::Read], Level::Full), 1, "Full shows as the highest offered");
        assert_eq!(active_level(&all, Level::Write), 2);
        assert_eq!(active_level(&all, Level::NoAccess), 0);
    }

    #[test]
    fn the_summary_names_the_template_or_the_areas() {
        assert_eq!(rights_summary(&RightsTemplate::ReadOnly.rights(), "codex"), tr("Read only"));
        assert_eq!(rights_summary(&RightsTemplate::FullAccess.rights(), "script"), tr("Full access"));
        assert_eq!(rights_summary(&Rights::hook(), HOOK_CLIENT), tr("Reports to the Notification Hub"));
        let mut rights = Rights::hook();
        rights.areas = BTreeMap::new();
        assert_eq!(rights_summary(&rights, "script"), tr("No access"));
        rights.areas.insert(Area::Calendar, AreaRights { level: Level::Read, admin: false });
        rights.areas.insert(Area::Inbox, AreaRights { level: Level::Write, admin: false });
        let areas = format!("{}, {}", tr("Calendar"), tr("Notification Hub"));
        assert_eq!(rights_summary(&rights, "codex"), trf("Custom: {areas}", &[("areas", &areas)]));
        let mut wide = RightsTemplate::ReadOnly.rights();
        wide.confirm = Confirm::Never;
        let first = format!("{}, {}, {}", tr("Workspaces"), tr("Calendar"), tr("Tasks"));
        // GitHub, Inbox, Mail, Accounts, AI, Tools, Settings, Apps follow.
        assert_eq!(rights_summary(&wide, "codex"), trf("Custom: {areas} +{n}", &[("areas", &first), ("n", &8)]));
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
    fn repositories_come_from_the_filter_lists_and_can_be_typed() {
        let data = data();
        assert_eq!(repos_in(&data.task_lists), ["anna/torro", "acme"], "-repo: is not offered");
        let selection = Selection::Only(vec!["ANNA/torro".into(), "zed/tool".into()]);
        let ids: Vec<String> = candidates(Pick::GithubRepos, &data, &selection).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["acme", "ANNA/torro", "zed/tool"], "each once as the selection spells it, sorted");
        assert_eq!(valid_repo(" anna/notes "), Some("anna/notes".into()));
        assert_eq!(valid_repo("acme"), Some("acme".into()));
        for bad in ["", "a/b/c", "/x", "x/", "two words", "a;b"] {
            assert_eq!(valid_repo(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn sources_include_every_client_and_the_selection_but_never_the_suite() {
        let mut data = data();
        data.sources.push(SourceInfo { source: SourceRef::suite(), open: 2, muted: false, health: None });
        let selection = Selection::Only(vec!["script:gone".into()]);
        let keys: Vec<String> = candidates(Pick::InboxSources, &data, &selection).into_iter().map(|c| c.id).collect();
        // Mail accounts are sources too (task E4.1), by their account id.
        assert_eq!(
            keys,
            [
                "agent:claude-code",
                "script:backup",
                "script:gone",
                "script:nightly",
                "mail:anna@mailbox.org",
                "mail:anna@work.example.net",
                // The Nextcloud account brings its notifications and Talk (task E5.14).
                "nextcloud:anna@cloud.example.org",
                "talk:anna@cloud.example.org"
            ]
        );

        // A selection from before still naming the suite shows it as gone, to untick it.
        let stale = Selection::Only(vec!["suite".into()]);
        let offered = candidates(Pick::InboxSources, &data, &stale);
        let suite = offered.iter().find(|candidate| candidate.id == "suite").expect("the stale entry");
        assert_eq!(suite.detail.as_deref(), Some(tr("No longer available")));
    }

    #[test]
    fn remote_accounts_offer_their_sources_by_kind_with_the_account_below() {
        let mut data = data();
        data.accounts.push(account("github:anna-dev", "github", "anna-dev"));
        data.accounts.push(account("ntfy:push.example.org", "ntfy", "anna@push.example.org"));
        data.accounts.push(account("caldav:anna@posteo.de@posteo.de", "caldav", "anna@posteo.de"));
        let offered = candidates(Pick::InboxSources, &data, &Selection::Only(Vec::new()));
        let remote: Vec<(&str, &str, &str)> = offered
            .iter()
            .filter(|c| ["github:", "nextcloud:", "talk:", "ntfy:"].iter().any(|kind| c.id.starts_with(kind)))
            .map(|c| (c.id.as_str(), c.label.as_str(), c.detail.as_deref().unwrap_or_default()))
            .collect();
        let github =
            trf("{account} · Also needs read access to GitHub and the repository", &[("account", &"anna-dev")]);
        assert_eq!(
            remote,
            [
                ("github:anna-dev", "GitHub", github.as_str()),
                ("nextcloud:anna@cloud.example.org", "Nextcloud", "anna@cloud.example.org"),
                ("talk:anna@cloud.example.org", "Talk", "anna@cloud.example.org"),
                ("ntfy:push.example.org", "ntfy", "push.example.org"),
            ],
            "a CalDAV account brings no source"
        );
        // A source with entries is offered once, not again for its account.
        data.sources.push(SourceInfo { source: SourceRef::github("anna-dev"), open: 3, muted: false, health: None });
        let again = candidates(Pick::InboxSources, &data, &Selection::All);
        assert_eq!(again.iter().filter(|c| c.id == "github:anna-dev").count(), 1);
    }

    #[test]
    fn segmented_controls_get_a_line_of_their_own_where_the_row_would_cut_them_off() {
        // The page widths of a narrow window (480, 16 margin) and of a medium one (800: rail,
        // inset and 24 margin).
        let narrow = 480.0 - 2.0 * space::l();
        let medium = 800.0 - torro_gui::shell::rail() - layout::inset() - 2.0 * space::xl();
        assert_eq!(control_column(narrow), None);
        assert_eq!(control_column(medium), Some(segment_slot()));
        assert_eq!(control_column(size::content_max()), Some(segment_slot()));
    }

    #[test]
    fn selected_ids_the_core_no_longer_knows_stay_to_be_unticked() {
        let data = data();
        let selection = Selection::Only(vec!["acc/work".into(), "acc/old".into()]);
        let offered = candidates(Pick::Calendars, &data, &selection);
        assert_eq!(offered.len(), 3);
        assert_eq!(offered[2].detail.as_deref(), Some(tr("No longer available")));
        assert!(offered[0].color.is_some(), "calendars keep their colour");
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
    fn every_segmented_control_fits_the_common_slot() {
        let slot = segment_slot();
        for area in Area::ALL.into_iter().filter(|area| area.grantable()) {
            let labels: Vec<&str> = levels(area).into_iter().map(widgets::level_label).collect();
            assert!(widgets::segmented_width(&labels) <= slot, "{area:?}");
        }
        assert!(widgets::segmented_width(&widgets::confirm_options().map(|(label, _)| label)) <= slot);
        assert!(widgets::segmented_width(&outbound_options().map(|(label, _)| label)) <= slot);
        assert!(widgets::segmented_width(&[tr("All"), tr("Selected")]) <= slot);
        assert!(slot < size::content_max() / 2.0, "leaves room for the area's name");
    }

    #[test]
    fn the_admin_switch_names_its_area() {
        let accounts = tr("Accounts");
        assert_eq!(admin_detail(Area::Accounts), trf("{area} · each time with your approval", &[("area", &accounts)]));
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
                let _ = update(&mut state, Message::All(pick, false), &data);
            }
            drop(view(&state, &data));
            let _ = update(&mut state, Message::AskDisconnect, &data);
            assert!(overlay(&state, &data).is_some());
        }
    }

    /// Task B.3: the assistant and the bots get no rows for `assistant` and `bots`; every other
    /// agent does, with a warning while one of them is on.
    #[test]
    fn bot_areas_are_hidden_where_they_do_nothing_and_warned_about_elsewhere() {
        for kind in [ASSISTANT_CLIENT, BOT_CLIENT] {
            assert!(!shows_area(Area::Assistant, kind) && !shows_area(Area::Bots, kind), "{kind}");
            assert!(shows_area(Area::Calendar, kind), "{kind}");
        }
        assert!(shows_area(Area::Bots, "codex") && shows_area(Area::Assistant, "codex"));

        let mut rights = RightsTemplate::ReadOnly.rights();
        assert_eq!(bot_warning(Area::Bots, &rights, "codex"), None, "off: no warning");
        rights.areas.insert(Area::Bots, AreaRights { level: Level::Write, admin: false });
        rights.areas.insert(Area::Assistant, AreaRights { level: Level::Read, admin: false });
        assert_eq!(bot_warning(Area::Bots, &rights, "codex"), Some(tr("Lets the agent act with a bot's rights.")));
        assert_eq!(bot_warning(Area::Assistant, &rights, "codex"), Some(tr("Also reads the conversations with bots.")));
        assert_eq!(bot_warning(Area::Calendar, &rights, "codex"), None);
        assert_eq!(bot_warning(Area::Bots, &rights, BOT_CLIENT), None, "a bot has no such row");
        assert_eq!(bot_warning(Area::Assistant, &rights, ASSISTANT_CLIENT), None);

        let bot = ClientInfo {
            id: "c0000001".into(),
            name: "Travel planner".into(),
            kind: BOT_CLIENT.into(),
            created: Utc::now(),
            last_used: None,
            rights: Rights::none(),
            agent: None,
        };
        assert_eq!(Category::of(&bot), Category::Bot);
        assert_eq!(kind_label(&bot), tr("Bot"));
        assert_eq!(kind_word(&bot), None, "the group says it");
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
        let _ = update(&mut state, Message::Level(Area::Tasks, Level::Write), &data);
        assert_eq!(state.choice.area(Area::Tasks).level, Level::Write);
        assert!(state.draft.is_none(), "nothing to save: there is no client yet");
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
        let _ = update(&mut state, Message::Level(Area::Calendar, Level::NoAccess), &data);
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
