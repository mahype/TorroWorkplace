//! Agents & API (task A.6 in `docs/tasks/api-mcp.md`, concept in `docs/api.md`, "Rechte" and
//! "Steuerzentrale: Abschnitt „Agents & API“"): the agents, hooks and scripts that may use the API,
//! and what each of them may do.
//!
//! - **List:** one settings group per kind of client (agents, hooks, scripts), a row per client
//!   with its name, what it is, its rights in words ([`rights_summary`]) and when it was last
//!   used; the row opens the detail page. Without clients an empty state that points to the
//!   header's "Connect agent", which opens the assistant of task A.7 (`agent_wizard.rs`).
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
//!   repositories, inbox sources and mail accounts ("All" or the switched-on ones, a calendar's
//!   colour as a dot as on the Accounts page; the suite's own entries are never offered, since the
//!   core shows them only to the person, task E1.2c) for each area the client can reach; its latest requests from
//!   the audit
//!   log (command and result, never contents); "Create new key" and "Disconnect…", each behind a
//!   confirmation that says what goes ([`disconnect_text`]). A new key is shown once, with a copy
//!   button.
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
//! detail page), `clients.new_key`, `clients.remove_client` and, from the assistant,
//! `clients.create_client`. The clients come from
//! `clients.list_clients`, the candidates of the selections from `calendar.list_calendars`,
//! `tasks.list_lists` (whose GitHub filter lists also name repositories), `inbox.list_sources` and
//! `accounts.list_accounts` (the accounts of the kind `mail`), all loaded by the shell into [`Data`]. Only the app key may send `clients.*`; the control centre
//! has it.

use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use iced::time::Instant;
use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{Space, center, column, container, responsive, row, svg, text_input, toggler};
use iced::{Alignment, Color, Element, Fill, Length, Subscription, Theme};
use torro_core::account::AccountKind;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Status, color_dot};
use torro_gui::{style, typo};
use torro_inbox::{AgentKind, SourceKind, SourceRef};
use torro_ipc::api::{
    self, ASSISTANT_CLIENT, Access, Area, AreaRights, CATALOGUE, Confirm, HOOK_CLIENT, Level, Mcp, Rights,
};
use torro_ipc::api::{Outbound, RightsTemplate, Selection};
use torro_ipc::{ActivityEntry, ClientInfo, IssuedKey, Request, Response};
use torro_pim::Collection;

use super::accounts::relative_time;
use super::{Action, Data, Header, Section, agent_wizard, chevron, confirm, dialog, pressable, row_frame, setting_row};
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

/// Page state.
#[derive(Debug, Default)]
pub struct State {
    /// The client whose detail page is open (its `ClientInfo::id`); `None` shows the list.
    pub selected: Option<String>,
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

/// One of the five selections of a client's rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Calendars,
    TaskLists,
    GithubRepos,
    InboxSources,
    MailAccounts,
}

impl Pick {
    pub const ALL: [Pick; 5] =
        [Pick::Calendars, Pick::TaskLists, Pick::GithubRepos, Pick::InboxSources, Pick::MailAccounts];

    /// The area whose level decides whether the selection matters at all.
    pub fn area(self) -> Area {
        match self {
            Pick::Calendars => Area::Calendar,
            Pick::TaskLists => Area::Tasks,
            Pick::GithubRepos => Area::Github,
            Pick::InboxSources => Area::Inbox,
            Pick::MailAccounts => Area::Mail,
        }
    }

    pub fn of(self, rights: &Rights) -> &Selection {
        match self {
            Pick::Calendars => &rights.calendars,
            Pick::TaskLists => &rights.task_lists,
            Pick::GithubRepos => &rights.github_repos,
            Pick::InboxSources => &rights.inbox_sources,
            Pick::MailAccounts => &rights.mail_accounts,
        }
    }

    fn of_mut(self, rights: &mut Rights) -> &mut Selection {
        match self {
            Pick::Calendars => &mut rights.calendars,
            Pick::TaskLists => &mut rights.task_lists,
            Pick::GithubRepos => &mut rights.github_repos,
            Pick::InboxSources => &mut rights.inbox_sources,
            Pick::MailAccounts => &mut rights.mail_accounts,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Pick::Calendars => tr("Calendars"),
            Pick::TaskLists => tr("Task lists"),
            Pick::GithubRepos => tr("GitHub repositories"),
            Pick::InboxSources => tr("Inbox sources"),
            Pick::MailAccounts => tr("Mail accounts"),
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
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Opens the detail page of a client.
    Open(String),
    /// The assistant's row in the list: its rights live on the Assistant page (task C.6).
    ShowAssistant,
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
    /// "Cancel" or Escape in a confirmation.
    CloseDialog,
    /// "Connect agent" in the header: opens the assistant.
    Connect,
    Wizard(agent_wizard::Message),
    /// A job the shell ran for the page ([`Action::Setup`]): detection and writing for the
    /// assistant, removing an entry after "Disconnect…".
    Setup(Outcome),
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Open(id) => {
            state.selected = Some(id.clone());
            state.dialog = None;
            state.repo_input.clear();
            state.activity = None;
            state.asked = Some(id.clone());
            Action::Send(Request::ClientActivity { id })
        }
        Message::Back => {
            state.selected = None;
            state.dialog = None;
            // Leaving the page is a good moment to save; the clock would do it a moment later.
            save_now(state)
        }
        Message::ShowAssistant => Action::Select(Section::Assistant),
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
            match selected_client(state, data) {
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
        Message::AskDisconnect => ask(state, data, Dialog::Disconnect),
        Message::Disconnect => {
            state.dialog = None;
            let Some(id) = state.selected.take() else { return Action::None };
            // Its rights need not be saved any more.
            if state.draft.as_ref().is_some_and(|draft| draft.id == id) {
                state.draft = None;
            }
            state.removing = Some(id.clone());
            Action::Send(Request::RemoveClient { id })
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
/// ([`change_rights`]) and waits [`SAVE_DELAY`] before saving. A hook client's rights are fixed:
/// nothing changes.
fn edit(state: &mut State, data: &Data, message: Message) -> Action {
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
/// reaches until the person unticks something.
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
            let admin = rights.area(area).admin;
            rights.areas.insert(area, AreaRights { level, admin });
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

/// Whether the rights view offers `area` for a client of `kind`: every grantable area
/// ([`Area::grantable`]), except `assistant` for the assistant's own client. The core refuses it that
/// area whatever its rights say ([`api::loops_back`]), so a switch there would promise what never
/// happens.
pub(super) fn shows_area(area: Area, kind: &str) -> bool {
    area.grantable() && !(kind == ASSISTANT_CLIENT && area == Area::Assistant)
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
        Ok(Response::ClientKey { client, key }) => {
            state.issued = Some(Issued { name: client.name.clone(), key: key.clone(), copied: false });
        }
        _ => {}
    }
    None
}

/// The shell loaded [`Data`] anew: a saved draft is in it now.
pub fn loaded(state: &mut State) {
    if state.draft.as_ref().is_some_and(|draft| draft.phase == Phase::Answered) {
        state.draft = None;
    }
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

/// The header bar: on a detail page the client's name with a back button to the list; on the list
/// "Connect agent", only its icon in narrow windows.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Header<'a, Message> {
    if let Some(client) = selected_client(state, data) {
        return Header { sub_page: Some((client.name.clone(), Message::Back)), actions: Vec::new() };
    }
    let connect = match class {
        SizeClass::Wide | SizeClass::Medium => {
            widgets::text_button(Some(Icon::Plus), tr("Connect agent"), style::button::primary, Some(Message::Connect))
        }
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
            widgets::compact_button(Icon::Plus, tr("Connect agent"), style::button::primary, Some(Message::Connect))
        }
    };
    Header { sub_page: None, actions: vec![connect] }
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
            confirm(title, consequence, tr("Disconnect"), Message::Disconnect, Message::CloseDialog)
        }
    })
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The client whose detail page is open, if it is still listed.
fn selected_client<'a>(state: &State, data: &'a Data) -> Option<&'a ClientInfo> {
    let id = state.selected.as_deref()?;
    data.clients.iter().find(|client| client.id == id)
}

/// The text typed into "Add repository" of the rights view ([`rights_groups`]).
pub(super) fn repo_input(state: &State) -> &str {
    &state.repo_input
}

/// The rights the page shows for `client`: its draft while there is one, else what the core holds.
pub(super) fn shown_rights<'a>(state: &'a State, client: &'a ClientInfo) -> &'a Rights {
    match &state.draft {
        Some(draft) if draft.id == client.id => &draft.rights,
        _ => &client.rights,
    }
}

/// Whether `area` has a command agents may use ([`Mcp::Tool`]) that needs `access`. Commands kept
/// from agents ([`Mcp::Never`]) are refused whatever the rights say, so they count for nothing.
fn has_command(area: Area, access: Access) -> bool {
    CATALOGUE.iter().any(|command| command.area == area && command.mcp == Mcp::Tool && command.access == access)
}

/// The levels worth offering in `area`, least first: always "None" and "Read", "Write" when the
/// area has commands that change something, "Full" when it has some that delete or overwrite.
/// So "Settings", whose changes are all admin commands, offers just "None" and "Read".
pub fn levels(area: Area) -> Vec<Level> {
    let mut levels = vec![Level::NoAccess, Level::Read];
    if has_command(area, Access::Write) {
        levels.push(Level::Write);
    }
    if has_command(area, Access::Delete) {
        levels.push(Level::Full);
    }
    levels
}

/// Whether `area` has admin commands agents may be allowed, so its admin switch is shown.
pub fn has_admin(area: Area) -> bool {
    has_command(area, Access::Admin)
}

/// The segment that shows `stored`: the highest offered level it reaches. A template gives "Full"
/// everywhere, which in an area without deleting commands allows what "Write" allows.
pub fn active_level(offered: &[Level], stored: Level) -> usize {
    offered.iter().rposition(|&level| level <= stored).unwrap_or(0)
}

/// The options of "Ask first", in the order the control shows them.
fn confirm_options() -> [(&'static str, Confirm); 3] {
    [
        (tr("Never"), Confirm::Never),
        (tr("On delete"), Confirm::OnDelete),
        (tr("On every change"), Confirm::OnEveryChange),
    ]
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

/// The estimated width of a segmented control (`widgets::segmented`) with `labels`: each label in
/// `CAPTION` with the button padding, plus the frame's inset and border. `typo::text_width`
/// estimates generously, so the control fits.
fn segmented_width(labels: &[&str]) -> f32 {
    let padding = style::button_padding().left + style::button_padding().right;
    let segments: f32 = labels.iter().map(|label| typo::text_width(label, typo::caption()) + padding).sum();
    segments + 2.0 * (style::SEGMENT_INSET + size::HAIRLINE)
}

/// The width of the slot every segmented control of the detail page starts in, left-aligned: as
/// wide as the widest of them (all four levels, "Ask first", "Send on my behalf", "All · Selected")
/// in the language
/// shown. So "None" stands under "None" whether an area offers two levels or four, and the page
/// lines up in every size class without a width of its own.
pub fn segment_slot() -> f32 {
    let levels = [Level::NoAccess, Level::Read, Level::Write, Level::Full].map(level_label);
    let confirm = confirm_options().map(|(label, _)| label);
    let outbound = outbound_options().map(|(label, _)| label);
    let pick = [tr("All"), tr("Selected")];
    [segmented_width(&levels), segmented_width(&confirm), segmented_width(&outbound), segmented_width(&pick)]
        .into_iter()
        .fold(0.0, f32::max)
}

/// `control` at the left of a slot `slot` wide ([`segment_slot`]).
fn in_slot<'a, M: 'a>(control: Element<'a, M>, slot: f32) -> Element<'a, M> {
    container(control).width(slot).into()
}

/// Where the controls of the detail page go on a page `width` wide: `Some(slot)`, a column
/// [`segment_slot`] wide at the right of every row, when the longest name and line of an area or
/// admin switch fit next to it in a `setting_row`; `None` when they would be cut off (a narrow
/// window, the narrow end of the medium one), then every segmented control stands on a line of its
/// own under its name ([`stacked_row`]) and the other controls go to the right edge. Decided by
/// what fits rather than by the size class, which the page does not know; it also holds for longer
/// translations.
pub fn control_column(width: f32) -> Option<f32> {
    let slot = segment_slot();
    let sending =
        [typo::text_width(tr("Send on my behalf"), typo::body()), typo::text_width(outbound_detail(), typo::meta())];
    let texts = Area::ALL.into_iter().filter(|area| area.grantable()).flat_map(|area| {
        let admin = has_admin(area).then(|| {
            [typo::text_width(admin_label(area), typo::body()), typo::text_width(&admin_detail(area), typo::meta())]
        });
        [typo::text_width(area_label(area), typo::body()), typo::text_width(area_detail(area), typo::meta())]
            .into_iter()
            .chain(admin.into_iter().flatten())
    });
    let text = texts.chain(sending).fold(0.0, f32::max);
    // A setting row: 16 at both sides and 16 between the text and the control.
    let row = 3.0 * space::l() + text + slot;
    (width >= row).then_some(slot)
}

/// `control` where the page puts its controls: at the left of the column, or as it is (at the
/// right edge of its row) without one ([`control_column`]).
pub(super) fn in_column<'a, M: 'a>(control: impl Into<Element<'a, M>>, column: Option<f32>) -> Element<'a, M> {
    match column {
        Some(slot) => in_slot(control.into(), slot),
        None => control.into(),
    }
}

/// A row with a segmented `control`: a `setting_row` with the control in the column, or without a
/// column the control on a line of its own ([`stacked_row`]).
fn segment_row<'a>(
    label: &'a str,
    detail: Option<String>,
    control: Element<'a, Message>,
    column: Option<f32>,
) -> Element<'a, Message> {
    match column {
        Some(slot) => setting_row(None, label, detail, in_slot(control, slot)),
        None => stacked_row(label, detail, control),
    }
}

/// `label`, `detail` below it and the segmented `control` on a line of its own under both, for a
/// page too narrow to put them side by side. The texts wrap rather than being cut off; 16 at the
/// sides as in every row of a group, 12 above and below.
fn stacked_row<'a>(label: &'a str, detail: Option<String>, control: Element<'a, Message>) -> Element<'a, Message> {
    let mut lines = column![typo::text(typo::body(), label)];
    if let Some(detail) = detail {
        lines = lines.push(typo::text(typo::meta(), detail).style(theme::text_2));
    }
    let content = column![lines, control].spacing(space::s());
    container(content).width(Fill).padding([space::m(), space::l()]).into()
}

fn level_label(level: Level) -> &'static str {
    match level {
        Level::NoAccess => tr("None"),
        Level::Read => tr("Read"),
        Level::Write => tr("Write"),
        Level::Full => tr("Full"),
    }
}

fn area_label(area: Area) -> &'static str {
    match area {
        Area::System => tr("System"),
        Area::Workspaces => tr("Workspaces"),
        Area::Calendar => tr("Calendar"),
        Area::Tasks => tr("Tasks"),
        Area::Github => tr("GitHub"),
        Area::Inbox => tr("Inbox"),
        Area::Mail => tr("Mail"),
        Area::Accounts => tr("Accounts"),
        Area::Ai => tr("AI"),
        Area::Assistant => tr("Assistant"),
        Area::Bots => tr("Bots"),
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
        return tr("Reports to the inbox").to_string();
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
    (Category::of(client) != Category::Script && kind != client.name).then_some(kind)
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

/// Title and consequence of "Disconnect…" for `client`. An agent loses its key and, where
/// "Connect agent" set them up, its configuration entry and inbox hooks ([`Job::Disconnect`]);
/// hooks are taken out of the agent's settings; a script only loses its key.
fn disconnect_text(client: &ClientInfo) -> (&'static str, String) {
    let name: &dyn std::fmt::Display = &client.name;
    match Category::of(client) {
        Category::Agent => (
            tr("Disconnect this agent?"),
            trf(
                "{name} loses access at once: its key stops working. Where Connect agent set them up, its configuration entry and its inbox hooks are removed too.",
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
        Category::Script => (
            tr("Disconnect this script?"),
            trf(
                "{name} loses access at once: its key stops working. Connect it again to give it access.",
                &[("name", name)],
            ),
        ),
    }
}

/// The groups of the list: agents, hooks, scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Agent,
    Hook,
    Script,
}

impl Category {
    const ALL: [Category; 3] = [Category::Agent, Category::Hook, Category::Script];

    fn of(client: &ClientInfo) -> Category {
        match client.kind.as_str() {
            HOOK_CLIENT => Category::Hook,
            "script" => Category::Script,
            _ => Category::Agent,
        }
    }

    /// One client of the group, for the line under its name on the detail page.
    fn singular(self) -> &'static str {
        match self {
            Category::Agent => tr("Agent"),
            Category::Hook => tr("Hook"),
            Category::Script => tr("Script"),
        }
    }

    fn title(self) -> &'static str {
        match self {
            Category::Agent => tr("Agents"),
            Category::Hook => tr("Hooks"),
            Category::Script => tr("Scripts"),
        }
    }

    fn footer(self) -> &'static str {
        match self {
            Category::Agent => {
                tr("Coding agents and apps that use Torro Workplace through MCP, each with its own key.")
            }
            Category::Hook => {
                tr("Hooks report to the inbox what an agent waits for or has finished. They see nothing.")
            }
            Category::Script => tr("Scripts and other programs that use the API with their own key."),
        }
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
/// account's id) and those `selection` names; each once, agents first, then scripts, then mail.
/// Never the suite: its entries (approvals, account problems) are only for the person, the core
/// shows them to no client whatever the selection says (task E1.2c). A selection that still names
/// it shows it as no longer available ([`candidates`]), so it can be unticked.
fn source_candidates(data: &Data, selection: &Selection) -> Vec<Candidate> {
    let mut sources: Vec<SourceRef> = data.sources.iter().map(|info| info.source.clone()).collect();
    sources.extend(data.clients.iter().filter_map(|client| api::post_source(Some(client), None).ok()));
    sources.extend(mail_candidates(data).iter().filter_map(|account| account.id.parse::<SourceRef>().ok()));
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
                SourceKind::Agent => tr("Agent"),
                SourceKind::Script | SourceKind::Suite => tr("Script"),
                SourceKind::Mail => tr("Mail"),
            };
            Candidate::plain(source.key(), source.label(), Some(detail.to_string()))
        })
        .collect()
}

/// The mail accounts an agent can be allowed (`accounts.list_accounts`, kind `mail`): the id is
/// what the core checks a call against ([`Rights::reaches_mail_account`]), the address is what the
/// person knows it by. A sign-in that is still running has no id and no mail yet.
fn mail_candidates(data: &Data) -> Vec<Candidate> {
    data.accounts
        .iter()
        .filter(|account| account.kind == AccountKind::Mail.name() && !account.id.is_empty())
        .map(|account| Candidate::plain(account.id.clone(), account.label.clone(), None))
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
        None => list(state, data, now),
    }
}

fn list<'a>(state: &'a State, data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
    let problem = state.problem.as_deref().map(problem_line);
    if data.clients.is_empty() {
        let empty = empty_state(
            tr("No agents connected"),
            tr(
                "Choose Connect agent to set up Claude Code, Codex, Gemini CLI, Claude Desktop or Cursor, or to create a key for a script.",
            ),
        );
        return column![].push(problem).push(empty).spacing(space::l()).into();
    }
    let groups = Category::ALL.into_iter().filter_map(|category| {
        let rows: Vec<Element<'a, Message>> = data
            .clients
            .iter()
            .filter(|client| Category::of(client) == category)
            .map(|client| client_row(state, client, now))
            .collect();
        (!rows.is_empty()).then(|| widgets::group(category.title(), rows, Some(category.footer().to_string())))
    });
    column![].push(problem).extend(groups).spacing(layout::section_gap()).into()
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
    // The assistant's rights are edited on its own page, next to its other settings.
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
    let name = typo::text(typo::headline(), client.name.as_str()).wrapping(Wrapping::None);
    let about = typo::text(typo::meta(), about(client, now)).style(theme::text_2);
    let heading = column![container(name).width(Fill).clip(true), about].spacing(space::xs());
    let mut page = column![heading].spacing(layout::section_gap());
    if client.kind == HOOK_CLIENT {
        page = page.push(hook_group(client, column));
    } else {
        let rights = shown_rights(state, client);
        page = page.extend(rights_groups(&state.repo_input, rights, data, column, &client.kind, tr("Access")));
    }
    let disconnect = widgets::text_button(None, tr("Disconnect…"), style::button::danger, Some(Message::AskDisconnect));
    page.push(activity_group(state, client, now)).push(key_group(column)).push(on_axis(disconnect)).into()
}

/// A text button moved left by its own padding, so its label stands on the page's content axis.
pub(super) fn on_axis<'a, M: 'a>(button: Element<'a, M>) -> Element<'a, M> {
    row![Space::new(), button].spacing(-style::button_padding().left).into()
}

/// The rights view of a client of `kind` (task A.6, shared with the Assistant page of task C.6): the
/// access group titled `title` ([`access_group`]), "Ask first", and the selection of each area the
/// rights reach (calendars, task lists, repositories, inbox sources, mail accounts).
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
    let mut groups = vec![access_group(rights, column, kind, title), confirm_group(rights.confirm, column)];
    for pick in Pick::ALL.into_iter().filter(|pick| rights.area(pick.area()).level != Level::NoAccess) {
        groups.push(selection_group(repo_input, pick, rights, data, column));
    }
    groups
}

/// The level of every area a client of `kind` can be given ([`shows_area`]), each followed by its
/// admin switch where it has one. The switch is a row of its own, not indented: admin commands are
/// allowed per area whatever the level (`docs/api.md`, "Rechte"), so its line names the area
/// instead.
fn access_group<'a>(rights: &Rights, column: Option<f32>, kind: &str, title: &'a str) -> Element<'a, Message> {
    let mut rows = Vec::new();
    for area in Area::ALL.into_iter().filter(|&area| shows_area(area, kind)) {
        let granted = rights.area(area);
        let offered = levels(area);
        let active = active_level(&offered, granted.level);
        let control = widgets::segmented(
            offered.into_iter().map(|level| (level_label(level), Message::Level(area, level))),
            active,
        );
        rows.push(segment_row(area_label(area), Some(area_detail(area).to_string()), control, column));
        if area == Area::Mail && can_send(granted.level) {
            rows.extend(outbound_rows(rights, column));
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
    let mut rows = vec![segment_row(tr("Send on my behalf"), Some(outbound_detail().to_string()), control, column)];
    if sends_freely(rights) {
        let mark = icon::icon(Icon::CircleAlert, icon::SIZE)
            .style(|t: &Theme, _| svg::Style { color: Some(palette(t).attention_text) });
        let words = typo::text(typo::meta(), tr("Mail can contain instructions aimed at agents. With Always, this agent sends without asking you first."))
            .style(theme::text_2);
        let content = row![mark, words.width(Fill)].spacing(space::m());
        rows.push(container(content).width(Fill).padding([space::m(), space::l()]).into());
    }
    rows
}

/// "Ask first": never, on delete, on every change.
fn confirm_group<'a>(current: Confirm, column: Option<f32>) -> Element<'a, Message> {
    let options = confirm_options();
    let active = options.iter().position(|(_, confirm)| *confirm == current).unwrap_or(0);
    let control = widgets::segmented(options.map(|(label, confirm)| (label, Message::Confirm(confirm))), active);
    let footer = tr("The agent waits until you allow it in Approvals. Admin commands always wait.");
    let row = segment_row(tr("Ask first"), None, control, column);
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
    let mut rows = vec![segment_row(tr("Access to"), None, switch, column)];
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
        (Pick::GithubRepos | Pick::InboxSources | Pick::MailAccounts, None) => None,
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
            value(level_label(Level::Write)),
        ),
    ];
    let footer = tr(
        "A hook's key sits in files the agent can read itself, so it may only report to the inbox and sees nothing else. These rights can't be changed.",
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
            sources: vec![SourceInfo { source: SourceRef::script("nightly"), open: 1, muted: false }],
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
        loaded(&mut state);
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
        assert_eq!(rights_summary(&Rights::hook(), HOOK_CLIENT), tr("Reports to the inbox"));
        let mut rights = Rights::hook();
        rights.areas = BTreeMap::new();
        assert_eq!(rights_summary(&rights, "script"), tr("No access"));
        rights.areas.insert(Area::Calendar, AreaRights { level: Level::Read, admin: false });
        rights.areas.insert(Area::Inbox, AreaRights { level: Level::Write, admin: false });
        let areas = format!("{}, {}", tr("Calendar"), tr("Inbox"));
        assert_eq!(rights_summary(&rights, "codex"), trf("Custom: {areas}", &[("areas", &areas)]));
        let mut wide = RightsTemplate::ReadOnly.rights();
        wide.confirm = Confirm::Never;
        let first = format!("{}, {}, {}", tr("Workspaces"), tr("Calendar"), tr("Tasks"));
        // GitHub, Inbox, Mail, Accounts, AI, Settings, Apps follow.
        assert_eq!(rights_summary(&wide, "codex"), trf("Custom: {areas} +{n}", &[("areas", &first), ("n", &7)]));
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
        data.sources.push(SourceInfo { source: SourceRef::suite(), open: 2, muted: false });
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
                "mail:anna@work.example.net"
            ]
        );

        // A selection from before still naming the suite shows it as gone, to untick it.
        let stale = Selection::Only(vec!["suite".into()]);
        let offered = candidates(Pick::InboxSources, &data, &stale);
        let suite = offered.iter().find(|candidate| candidate.id == "suite").expect("the stale entry");
        assert_eq!(suite.detail.as_deref(), Some(tr("No longer available")));
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
            let labels: Vec<&str> = levels(area).into_iter().map(level_label).collect();
            assert!(segmented_width(&labels) <= slot, "{area:?}");
        }
        assert!(segmented_width(&confirm_options().map(|(label, _)| label)) <= slot);
        assert!(segmented_width(&outbound_options().map(|(label, _)| label)) <= slot);
        assert!(segmented_width(&[tr("All"), tr("Selected")]) <= slot);
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
            "{name} loses access at once: its key stops working. Where Connect agent set them up, its configuration entry and its inbox hooks are removed too.",
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
}
