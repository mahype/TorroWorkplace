//! MCP (task S.35 in `docs/tasks/steuerzentrale.md`, concept in `docs/settings.md` › "MCP"):
//! coding agents and apps that use Torro Workplace through `torroworkplace mcp`, each with its own
//! key, and the hooks with which an agent reports to the Notification Hub.
//!
//! - **"MCP server":** the switch "Allow MCP" (`settings.set_setting allow_mcp`). Off, the core
//!   refuses every MCP client and its hooks; only a person turns it on again
//!   (`Request::kept_for_people`). Below it the command a client runs, to copy for one set up by
//!   hand ([`client_setup::mcp_command`], the key in `TORRO_TOKEN`).
//! - **"Connected clients":** a row per MCP client (`api::MCP_KINDS`: Claude Code, Codex, Gemini
//!   CLI, Claude Desktop, Cursor, "Other MCP client"), which opens its page, and a row "Hooks
//!   without an agent" for every hook client whose agent is gone ([`orphans`]), with "Remove".
//!   "Connect client" in the header opens the dialog of `agent_wizard.rs` ([`agent_wizard::State::new`]).
//! - **A client's page:** the shared page of `api.rs` ([`Page`]) with the commands named as MCP
//!   tools, and for Claude Code, Codex and Gemini CLI the group "Notification Hub" between the
//!   name and the rights – one switch, which would be screens down below the editor: "Report when
//!   waiting or done" and where the hooks are.
//!
//! **Hooks** are a client of their own (kind `hook`, `Rights::hook`), but no list shows them: the
//! switch on the agent's page makes and removes them. On: `clients.create_client` for a hook client
//! of the agent, then the shell writes the hooks with its key ([`Job::Hook`], the key goes straight
//! into the job). Off: `clients.remove_client` for the hook client, then [`Job::Disconnect`] for it,
//! which takes out only its hooks. Which hook client reports for which agent client, and where its
//! hooks are, comes from `client_setup.json` ([`Job::HookNotes`], read when the section opens and
//! after every change); a hook client noted without its agent (older versions) or not at all
//! belongs to the first MCP client of its agent's kind ([`owner_of`]).
//!
//! "Disconnect…" also takes Torro Workplace out of the agent's configuration when "Connect client"
//! put it there: once the core confirmed `clients.remove_client`, the shell runs
//! [`Job::Disconnect`], which looks up what the dialog noted for that client and does nothing for
//! any other. A failure shows above the list. The agent's hooks go with it: the job takes them out
//! of the agent's settings and names their hook client, which the page then removes from the core
//! as well (`clients.remove_client`, one after the other).
//!
//! Requests: `settings.set_setting` (the switch), `clients.create_client` ("Connect client", the
//! hook switch), `clients.update_client`, `clients.activity`, `clients.new_key` and
//! `clients.remove_client` (through [`Page`], and for hook clients). Only the app key may send
//! `clients.*`; the control centre has it. Every job runs in the shell ([`Action::Setup`]), whose
//! outcome comes back as [`Message::Setup`].

use chrono::{DateTime, Utc};
use iced::widget::{Space, column, container, responsive, svg, toggler};
use iced::{Color, Element, Fill, Length, Subscription, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{Palette, palette, size, space};
use torro_gui::{style, widgets};
use torro_inbox::AgentKind;
use torro_ipc::api::{HOOK_CLIENT, RightsTemplate, is_mcp_kind};
use torro_ipc::{ClientInfo, Request, Response, SettingKey};
use torro_settings_ui::rights::{control_column, in_column};

use super::api::{self, Home, Page, PageMessage, add_row, allow_row, primary};
use super::{Action, Data, Header, agent_wizard, confirm, dialog, setting_row};
use crate::client_setup::{self, HookNote, Job, Outcome, Target};

/// Page state of MCP.
#[derive(Debug)]
pub struct State {
    /// A client's page, or the list.
    pub page: Page,
    /// "Connect client" while it is open.
    pub wizard: Option<agent_wizard::State>,
    /// Why taking Torro Workplace out of an agent's configuration failed, shown above the list.
    problem: Option<String>,
    /// Hook clients of a disconnected agent still to remove from the core.
    orphans: Vec<String>,
    /// What `client_setup.json` notes of the hooks; `None` until it was read.
    pub notes: Option<Vec<HookNote>>,
    /// Why `client_setup.json` could not be read, shown above the list.
    notes_problem: Option<String>,
    /// The switch "Report when waiting or done" on its way.
    change: Option<Change>,
    /// Why the switch last failed, for the page of the client it was flipped on.
    hook_problem: Option<HookProblem>,
    /// The hook client whose "Remove" waits for its confirmation.
    confirm_orphan: Option<String>,
    /// The command an MCP client runs ([`client_setup::mcp_command`]).
    command: String,
    /// "Copy" next to the command was pressed.
    copied: bool,
}

impl Default for State {
    fn default() -> Self {
        let program = std::env::current_exe().unwrap_or_else(|_| torro_core::AppId::WORKPLACE.binary.into());
        State {
            page: Page::new(Home::Mcp),
            wizard: None,
            problem: None,
            orphans: Vec::new(),
            notes: None,
            notes_problem: None,
            change: None,
            hook_problem: None,
            confirm_orphan: None,
            command: client_setup::mcp_command(&program),
            copied: false,
        }
    }
}

/// A flip of "Report when waiting or done" on its way, for the agent client `owner`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    /// `clients.create_client` for the hook client of `agent`.
    Creating { owner: String, agent: AgentKind },
    /// The shell writes the hooks of the new hook client.
    Writing { owner: String, hook: String },
    /// `clients.remove_client` for the hook client `hook`, whose hooks are in `place`. An answer
    /// cannot say which request it belongs to, so the hooks are taken out once a reload no longer
    /// lists the hook client ([`loaded`]); `refused` keeps an error that came meanwhile, which ends
    /// the flip if the reload still lists it.
    Removing { owner: String, hook: String, place: Option<String>, refused: Option<String> },
    /// The shell takes out the hooks of the removed hook client.
    Unhooking { owner: String, place: Option<String> },
}

impl Change {
    fn owner(&self) -> &str {
        match self {
            Change::Creating { owner, .. }
            | Change::Writing { owner, .. }
            | Change::Removing { owner, .. }
            | Change::Unhooking { owner, .. } => owner,
        }
    }

    /// Where the switch goes: on while hooks are set up, off while they are taken out.
    fn on(&self) -> bool {
        matches!(self, Change::Creating { .. } | Change::Writing { .. })
    }
}

/// Why a flip of the switch failed: on the page of `client`, setting up (`on`) or taking out the
/// hooks in `place`, which then names where to take them out by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HookProblem {
    client: String,
    on: bool,
    why: String,
    place: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Page(PageMessage),
    /// The switch "Allow MCP".
    Allow(bool),
    /// "Copy" next to the command.
    CopyCommand,
    /// Typing into the command's field changes nothing.
    KeepCommand,
    /// "Connect client" in the header: opens the dialog and asks for detection.
    Connect,
    Wizard(agent_wizard::Message),
    /// The switch "Report when waiting or done" on the open page.
    Report(bool),
    /// "Remove" of a row "Hooks without an agent", then its confirmation.
    AskRemoveHooks(String),
    RemoveHooks,
    /// "Cancel" or Escape in that confirmation.
    CloseDialog,
    /// A job the shell ran for the section ([`Action::Setup`]).
    Setup(Outcome),
}

/// What opening the section does: read the hook notes anew, since another window or a disconnect
/// may have changed them.
pub fn opened() -> Action {
    Action::Setup(Job::HookNotes)
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Page(message) => state.page.update(message, data),
        // Turning it on is the app key's only (`Request::kept_for_people`), which this window has.
        Message::Allow(on) => {
            Action::Send(Request::SetSetting { key: SettingKey::AllowMcp, value: serde_json::Value::Bool(on) })
        }
        Message::CopyCommand => {
            state.copied = true;
            Action::Copy(state.command.clone())
        }
        Message::KeepCommand => Action::None,
        Message::Connect => {
            state.wizard = Some(agent_wizard::State::new());
            state.problem = None;
            Action::Setup(Job::Detect)
        }
        Message::Wizard(message) => agent_wizard::update_in(&mut state.wizard, message, data),
        Message::Report(on) => report(state, on, data),
        Message::AskRemoveHooks(id) => {
            state.confirm_orphan = orphans(data, state.notes.as_deref()).any(|hook| hook.id == id).then_some(id);
            Action::None
        }
        Message::RemoveHooks => match state.confirm_orphan.take() {
            Some(id) if !state.page.is_removing() => state.page.remove(id),
            _ => Action::None,
        },
        Message::CloseDialog => {
            state.confirm_orphan = None;
            Action::None
        }
        Message::Setup(outcome) => setup(state, outcome, data),
    }
}

/// The switch "Report when waiting or done" on the open page: on creates the hook client of the
/// agent (`clients.create_client`, kind `hook`, the template "Full access", which the core narrows
/// to `Rights::hook`), off removes the one that reports for it. Nothing for a client without hooks,
/// when the switch already stands there, or while it is held ([`held`]).
fn report(state: &mut State, on: bool, data: &Data) -> Action {
    let Some(client) = state.page.client(data) else { return Action::None };
    let Some(agent) = hook_agent(&client.kind) else { return Action::None };
    if held(state) {
        return Action::None;
    }
    state.hook_problem = None;
    let owner = client.id.clone();
    match (on, hook_of(client, data, state.notes.as_deref())) {
        (true, None) => {
            state.change = Some(Change::Creating { owner, agent });
            Action::Send(Request::CreateClient {
                name: trf("{agent} hooks", &[("agent", &agent.label())]),
                kind: HOOK_CLIENT.to_string(),
                template: RightsTemplate::FullAccess,
                agent: Some(agent),
                rights: None,
            })
        }
        (false, Some(hook)) => {
            let hook = hook.id.clone();
            let place = state.notes.iter().flatten().find(|note| note.client == hook).map(|note| note.place.clone());
            state.change = Some(Change::Removing { owner, hook: hook.clone(), place, refused: None });
            Action::Send(Request::RemoveClient { id: hook })
        }
        _ => Action::None,
    }
}

/// Whether the switch "Report when waiting or done" is held: while a flip is on its way, until the
/// hook notes were read (without them the hook client of the page is not known for sure), and while
/// a client or the hook clients of a disconnected agent are being removed – their answers look like
/// the switch's.
fn held(state: &State) -> bool {
    state.change.is_some() || state.notes.is_none() || state.page.is_removing() || !state.orphans.is_empty()
}

/// A job's outcome: detection and the dialog's own jobs go to "Connect client"; written or taken
/// out hooks end a flip of the switch (a failure with its reason, a hook client whose hooks could
/// not be written removed again); a disconnect goes on with the next hook client of the agent; the
/// notes are kept.
fn setup(state: &mut State, outcome: Outcome, data: &Data) -> Action {
    match (outcome, state.change.clone()) {
        (Outcome::HookNotes(notes), _) => {
            // An unreadable record leaves no notes: hook clients then belong to their agent's kind,
            // and the list says why.
            state.notes_problem = notes.as_ref().err().cloned();
            state.notes = Some(notes.unwrap_or_default());
            Action::None
        }
        (Outcome::Hooked(result), Some(Change::Writing { owner, hook })) => {
            state.change = None;
            match result {
                Ok(_) => opened(),
                Err(why) => {
                    state.hook_problem = Some(HookProblem { client: owner, on: true, why, place: None });
                    // Nothing uses its key.
                    Action::Send(Request::RemoveClient { id: hook })
                }
            }
        }
        (Outcome::Disconnected(result), Some(Change::Unhooking { owner, place })) => {
            state.change = None;
            if let Err(why) = result {
                state.hook_problem = Some(HookProblem { client: owner, on: false, why, place });
            }
            opened()
        }
        (Outcome::Disconnected(result), _) => {
            match result {
                Ok(released) => {
                    state.problem = None;
                    state.orphans.extend(released.hook_clients);
                }
                Err(problem) => state.problem = Some(problem),
            }
            remove_orphan(state, data)
        }
        (outcome, _) => agent_wizard::update_in(&mut state.wizard, agent_wizard::Message::Setup(outcome), data),
    }
}

/// Removes the next hook client of a disconnected agent, the way "Disconnect…" removes a client:
/// once the core confirms, its hooks are looked up too ([`answered`]), and the next one follows.
/// With none left, the notes are read anew.
fn remove_orphan(state: &mut State, data: &Data) -> Action {
    if state.page.is_removing() {
        return Action::None;
    }
    while let Some(id) = state.orphans.pop() {
        if data.clients.iter().any(|client| client.id == id) {
            return state.page.remove(id);
        }
    }
    opened()
}

/// The answer to a request the shell sent (it calls this for every answer, then reloads):
/// "Connect client"'s own answers first ([`agent_wizard::answered`]), then a flip of the switch –
/// the new hook client's key goes straight into [`Job::Hook`] –, then the page's
/// ([`Page::answered`]): a confirmed removal takes the client out of its agent's configuration too
/// ([`Job::Disconnect`]). Returns `Some` with what to do next when the answer was the section's
/// (the shell then shows no notice of its own).
pub fn answered(state: &mut State, result: &Result<Response, String>) -> Option<Action> {
    if let Some(next) = state.wizard.as_mut().and_then(|wizard| agent_wizard::answered(wizard, result)) {
        return Some(next);
    }
    if let Some(next) = switched(state, result) {
        return Some(next);
    }
    state.page.answered(result).map(|client| Action::Setup(Job::Disconnect { client }))
}

/// The answer to a flip of the switch on its way, `None` when it is not one. The new hook client's
/// key names its kind and agent; an error while the hook client is created ends the flip. An error
/// while it is removed is only kept ([`Change::Removing`]): it may be another request's, so the
/// shell shows it too.
fn switched(state: &mut State, result: &Result<Response, String>) -> Option<Action> {
    match (state.change.clone()?, result) {
        (Change::Creating { owner, agent }, Ok(Response::ClientKey { client, key }))
            if client.kind == HOOK_CLIENT && client.agent == Some(agent) =>
        {
            state.change = Some(Change::Writing { owner: owner.clone(), hook: client.id.clone() });
            Some(Action::Setup(Job::Hook { agent, owner: Some(owner), client: client.id.clone(), key: key.clone() }))
        }
        (Change::Creating { owner, .. }, Err(why)) => {
            state.change = None;
            state.hook_problem = Some(HookProblem { client: owner, on: true, why: why.clone(), place: None });
            Some(Action::None)
        }
        (Change::Removing { owner, hook, place, .. }, Err(why)) => {
            state.change = Some(Change::Removing { owner, hook, place, refused: Some(why.clone()) });
            None
        }
        _ => None,
    }
}

/// The shell loaded [`Data`] anew. While the switch removes a hook client: once the core no longer
/// lists it, its hooks are taken out (the returned [`Job::Disconnect`], which takes out only its
/// own); still listed after an error, the flip ends with that error.
pub fn loaded(state: &mut State, data: &Data) -> Option<Action> {
    state.page.loaded();
    let Some(Change::Removing { owner, hook, place, refused }) = state.change.clone() else { return None };
    if data.error.is_some() {
        return None;
    }
    if !data.clients.iter().any(|client| client.id == hook) {
        state.change = Some(Change::Unhooking { owner, place });
        return Some(Action::Setup(Job::Disconnect { client: hook }));
    }
    if let Some(why) = refused {
        state.change = None;
        state.hook_problem = Some(HookProblem { client: owner, on: false, why, place: None });
    }
    None
}

pub fn subscription(state: &State) -> Subscription<Message> {
    state.page.subscription().map(Message::Page)
}

/// What Escape means here right now: close "Connect client" (unless it is creating the client),
/// the confirmation of "Remove", the key dialog or the page's confirmation.
pub fn escape(state: &State) -> Option<Message> {
    if let Some(wizard) = &state.wizard {
        return agent_wizard::closable(wizard).then_some(Message::Wizard(agent_wizard::Message::Close));
    }
    if state.confirm_orphan.is_some() {
        return Some(Message::CloseDialog);
    }
    state.page.escape().map(Message::Page)
}

/// The header bar: on a client's page its name with a back button; on the list "Connect client",
/// the window's one primary button.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Header<'a, Message> {
    match state.page.header(data) {
        Some(header) => header.map(Message::Page),
        None => Header { sub_page: None, actions: vec![primary(class, tr("Connect client"), Message::Connect)] },
    }
}

/// The section's dialog over the whole window: "Connect client", the confirmation of "Remove",
/// else the page's.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(wizard) = &state.wizard {
        let content = agent_wizard::view(wizard, data).map(Message::Wizard);
        return Some(dialog(content, size::form_max(), Message::Wizard(agent_wizard::Message::Dismiss)));
    }
    if let Some(id) = &state.confirm_orphan {
        let hook = data.clients.iter().find(|client| client.id == *id)?;
        let name: &dyn std::fmt::Display = &hook.name;
        return Some(confirm(
            tr("Disconnect these hooks?"),
            trf(
                "{name} stop reporting at once: their key stops working and they are taken out of the agent's settings.",
                &[("name", name)],
            ),
            tr("Remove"),
            Message::RemoveHooks,
            Message::CloseDialog,
        ));
    }
    state.page.overlay(data).map(|overlay| overlay.map(Message::Page))
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The agent whose hooks can report for a client of `kind`: Claude Code, Codex and Gemini CLI
/// ([`Target::agent`]); `None` for every other kind.
pub fn hook_agent(kind: &str) -> Option<AgentKind> {
    Target::ALL.into_iter().find(|target| target.kind() == kind).and_then(Target::agent)
}

/// The MCP client the hook client `hook` reports for: the one `notes` name as its owner, or – for a
/// hook client noted without an owner (older versions) or not noted at all – the first MCP client
/// of its agent's kind. `None` when that client is gone: then the hooks have no agent.
pub fn owner_of<'a>(hook: &ClientInfo, data: &'a Data, notes: Option<&[HookNote]>) -> Option<&'a ClientInfo> {
    let mcp = || data.clients.iter().filter(|client| is_mcp_kind(&client.kind));
    let noted = notes.unwrap_or_default().iter().find(|note| note.client == hook.id);
    match noted.and_then(|note| note.owner.as_deref()) {
        Some(owner) => mcp().find(|client| client.id == owner),
        None => {
            let agent = hook.agent?;
            mcp().find(|client| client.kind == agent.id())
        }
    }
}

/// The hook client that reports for `client`, if any ([`owner_of`]).
pub fn hook_of<'a>(client: &ClientInfo, data: &'a Data, notes: Option<&[HookNote]>) -> Option<&'a ClientInfo> {
    hooks(data).find(|hook| owner_of(hook, data, notes).is_some_and(|owner| owner.id == client.id))
}

/// The hook clients whose agent is gone ([`owner_of`] finds none): "Hooks without an agent".
pub fn orphans<'a>(data: &'a Data, notes: Option<&'a [HookNote]>) -> impl Iterator<Item = &'a ClientInfo> {
    hooks(data).filter(move |hook| owner_of(hook, data, notes).is_none())
}

fn hooks(data: &Data) -> impl Iterator<Item = &ClientInfo> {
    data.clients.iter().filter(|client| client.kind == HOOK_CLIENT)
}

/// Where the switch "Report when waiting or done" of `client` stands: where a flip on its way
/// takes it, else whether a hook client reports for it.
fn reports(state: &State, client: &ClientInfo, data: &Data) -> bool {
    match &state.change {
        Some(change) if change.owner() == client.id => change.on(),
        _ => hook_of(client, data, state.notes.as_deref()).is_some(),
    }
}

// ----- Views --------------------------------------------------------------------------------

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let now = Utc::now();
    match state.page.client(data) {
        Some(client) => responsive(move |size| detail(state, client, data, now, control_column(size.width)))
            .height(Length::Shrink)
            .into(),
        None => list(state, data, now),
    }
}

/// The list: why an agent's configuration still holds its entry, the server with its switch and
/// command, then the clients and the hooks without an agent.
fn list<'a>(state: &'a State, data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
    let problem = state.problem.as_deref().map(|problem| {
        super::notice(trf(
            "Torro Workplace could not be taken out of the agent's configuration: {problem}",
            &[("problem", &problem)],
        ))
    });
    let unread = state.notes_problem.as_deref().map(|problem| {
        super::notice(trf("What Connect client set up could not be read: {problem}", &[("problem", &problem)]))
    });
    let refused = !data.settings.allow_mcp;
    let mut rows: Vec<Element<'a, Message>> =
        api::listed(data, Home::Mcp).map(|client| state.page.row(client, now, refused).map(Message::Page)).collect();
    rows.extend(orphans(data, state.notes.as_deref()).map(orphan_row));
    if rows.is_empty() {
        rows.push(add_row(tr("Connect client…"), Message::Connect));
    }
    let footer = tr("Coding agents and apps that use Torro Workplace through MCP, each with its own key.");
    let clients = widgets::group(tr("Connected clients"), rows, Some(footer.to_string()));
    column![]
        .push(problem)
        .push(unread)
        .push(server_group(state, data))
        .push(clients)
        .spacing(layout::section_gap())
        .into()
}

/// "MCP server": the switch and, under "Command", the command to copy – its row as high as the
/// label and the field need, 12 above and below like a wrapping row.
fn server_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let allow = allow_row(
        tr("Allow MCP"),
        data.settings.allow_mcp,
        tr("Connected clients can use Torro Workplace through MCP"),
        tr("Every MCP client and its hooks are refused"),
        Message::Allow,
    );
    let command = agent_wizard::command_line(&state.command, state.copied, Message::CopyCommand, Message::KeepCommand);
    let command = container(command).width(Fill).padding([space::m(), space::l()]).into();
    let footer = trf(
        "The command for a client you set up yourself; it takes the client's key as {variable}. Only you can turn MCP on again.",
        &[("variable", &client_setup::TOKEN_VARIABLE)],
    );
    widgets::group(tr("MCP server"), vec![allow, command], Some(footer))
}

/// A hook client whose agent is gone: what it is, and "Remove".
fn orphan_row(hook: &ClientInfo) -> Element<'_, Message> {
    let remove = widgets::text_button(
        None,
        tr("Remove"),
        style::button::secondary,
        Some(Message::AskRemoveHooks(hook.id.clone())),
    );
    setting_row(None, tr("Hooks without an agent"), Some(api::kind_label(hook)), remove)
}

/// A client's page: the shared head of [`Page`] – for an agent with hooks the group "Notification
/// Hub" between its name and the rights, so the one switch is not screens down –, then the shared
/// foot; the controls in `slot`.
fn detail<'a>(
    state: &'a State,
    client: &'a ClientInfo,
    data: &'a Data,
    now: DateTime<Utc>,
    slot: Option<f32>,
) -> Element<'a, Message> {
    let page = &state.page;
    let wrap = |part: Element<'a, PageMessage>| part.map(Message::Page);
    let mut head = page.head(client, data, now, slot).into_iter().map(wrap);
    let mut parts: Vec<Element<'a, Message>> = head.next().into_iter().collect();
    parts.extend(hook_agent(&client.kind).map(|agent| hub_group(state, client, agent, data, slot)));
    parts.extend(head);
    parts.extend(page.foot(client, now, slot).into_iter().map(wrap));
    column(parts).spacing(layout::section_gap()).into()
}

/// "Notification Hub": the switch "Report when waiting or done" – off while a flip is on its way –
/// and below it how the hooks stand: where they are (Codex runs them only once they are trusted),
/// or why the last flip failed.
fn hub_group<'a>(
    state: &'a State,
    client: &'a ClientInfo,
    agent: AgentKind,
    data: &'a Data,
    slot: Option<f32>,
) -> Element<'a, Message> {
    let on = reports(state, client, data);
    let mut switch = toggler(on).size(style::CHECK_SIZE).style(style::toggler);
    if !held(state) {
        switch = switch.on_toggle(Message::Report);
    }
    let detail = tr("A separate key that can only report");
    let mut rows =
        vec![setting_row(None, tr("Report when waiting or done"), Some(detail.to_string()), in_column(switch, slot))];
    let problem = state.hook_problem.as_ref().filter(|problem| problem.client == client.id);
    let hook = hook_of(client, data, state.notes.as_deref());
    let note = hook.and_then(|hook| state.notes.iter().flatten().find(|note| note.client == hook.id));
    match (problem, note) {
        (Some(problem), _) => {
            let title = match (problem.on, &problem.place) {
                (true, _) => tr("Notification Hub reports could not be set up").to_string(),
                (false, Some(file)) => trf("The hooks could not be taken out of {file}", &[("file", file)]),
                (false, None) => tr("The hooks could not be taken out of the agent's settings").to_string(),
            };
            rows.push(setting_row(
                Some(mark(Icon::CircleX, |p| p.error)),
                title,
                Some(problem.why.clone()),
                Space::new(),
            ));
        }
        (None, Some(note)) if state.change.is_none() => {
            let added = trf("Hooks added to {file}", &[("file", &note.place)]);
            let trust = (agent == AgentKind::Codex)
                .then(|| tr("Codex runs them only after you trust them: type /hooks in Codex").to_string());
            rows.push(setting_row(Some(mark(Icon::CircleCheck, |p| p.ok)), added, trust, Space::new()));
        }
        _ => {}
    }
    let footer = tr("Hooks report to the Notification Hub what an agent waits for or has finished. They see nothing.");
    widgets::group(tr("Notification Hub"), rows, Some(footer.to_string()))
}

/// A status icon in front of a row: 16, in the colour `colour` picks from the palette.
fn mark<'a>(which: Icon, colour: fn(&Palette) -> Color) -> Element<'a, Message> {
    icon::icon(which, icon::SIZE).style(move |t: &Theme, _| svg::Style { color: Some(colour(&palette(t))) }).into()
}

#[cfg(test)]
mod tests {
    use torro_ipc::api::{MCP_CLIENT, Rights};
    use torro_ipc::{IssuedKey, Key};

    use super::super::api::tests::{client, data};
    use super::*;
    use crate::client_setup::Released;

    fn note(hook: &str, owner: Option<&str>) -> HookNote {
        HookNote {
            client: hook.into(),
            owner: owner.map(str::to_string),
            agent: AgentKind::ClaudeCode,
            place: "~/.claude/settings.json".into(),
        }
    }

    /// MCP with the hook notes read and the page of `id` open.
    fn opened(id: &str, data: &Data, notes: Vec<HookNote>) -> State {
        let mut state = State::default();
        let _ = update(&mut state, Message::Setup(Outcome::HookNotes(Ok(notes))), data);
        let action = update(&mut state, Message::Page(PageMessage::Open(id.into())), data);
        assert!(matches!(action, Action::Send(Request::ClientActivity { id: ref asked }) if asked == id));
        state
    }

    fn hook_key(id: &str, agent: AgentKind) -> Result<Response, String> {
        let mut hook = client(id, "Claude Code hooks", HOOK_CLIENT, Rights::hook());
        hook.agent = Some(agent);
        Ok(Response::ClientKey { client: hook, key: IssuedKey::new(Key::new("torro_h2_secret")) })
    }

    /// Task S.35: a hook client belongs to the agent client its note names; without a note naming
    /// one, to the first MCP client of its agent's kind; with its agent gone it has none and shows
    /// as "Hooks without an agent".
    #[test]
    fn hooks_belong_to_the_agent_their_note_names() {
        let mut data = data();
        let second = client("c2", "Laptop agent", "claude-code", Rights::none());
        data.clients.push(second);
        let (first, second, hook) = (&data.clients[0], &data.clients[5], &data.clients[1]);
        let noted = [note("h1", Some("c2"))];
        assert_eq!(owner_of(hook, &data, Some(&noted)).map(|c| c.id.as_str()), Some("c2"));
        assert_eq!(hook_of(second, &data, Some(&noted)).map(|h| h.id.as_str()), Some("h1"));
        assert!(hook_of(first, &data, Some(&noted)).is_none());
        // Older notes without an owner, or none at all: the first client of the agent's kind.
        for notes in [None, Some(&[note("h1", None)][..])] {
            assert_eq!(owner_of(hook, &data, notes).map(|c| c.id.as_str()), Some("c1"));
        }
        assert_eq!(orphans(&data, Some(&noted)).count(), 0);
        let gone = [note("h1", Some("c9"))];
        assert_eq!(orphans(&data, Some(&gone)).map(|h| h.id.as_str()).collect::<Vec<_>>(), ["h1"]);
        assert_eq!(hook_agent("claude-code"), Some(AgentKind::ClaudeCode));
        assert_eq!(hook_agent("gemini"), Some(AgentKind::Gemini));
        assert_eq!(hook_agent("cursor"), None);
        assert_eq!(hook_agent(MCP_CLIENT), None);
    }

    /// Task S.35: the switch "Report when waiting or done" on – a hook client for the agent, its
    /// key straight into the job that writes the hooks, the notes read anew – and off – the hook
    /// client removed, then only its hooks taken out.
    #[test]
    fn the_switch_creates_and_removes_the_hook_client_and_its_hooks() {
        let mut data = data();
        data.clients.retain(|client| client.kind != HOOK_CLIENT);
        let mut state = opened("c1", &data, Vec::new());
        let claude = data.clients[0].clone();
        assert!(!reports(&state, &claude, &data));
        let action = update(&mut state, Message::Report(true), &data);
        let expected_name = trf("{agent} hooks", &[("agent", &"Claude Code")]);
        assert!(matches!(
            action,
            Action::Send(Request::CreateClient {
                ref name,
                ref kind,
                template: RightsTemplate::FullAccess,
                agent: Some(AgentKind::ClaudeCode),
                rights: None,
            }) if *name == expected_name && kind == HOOK_CLIENT
        ));
        assert!(reports(&state, &claude, &data), "the switch shows where it goes at once");
        assert!(matches!(update(&mut state, Message::Report(false), &data), Action::None), "one flip at a time");
        drop(view(&state, &data));

        match answered(&mut state, &hook_key("h2", AgentKind::ClaudeCode)) {
            Some(Action::Setup(Job::Hook { agent, owner, client, key })) => {
                assert_eq!((agent, owner.as_deref(), client.as_str()), (AgentKind::ClaudeCode, Some("c1"), "h2"));
                assert_eq!(key.expose(), "torro_h2_secret");
            }
            other => panic!("expected the hooks to be written, got {other:?}"),
        }
        assert!(state.page.overlay(&data).is_none(), "the hook key is never shown");
        let written = update(&mut state, Message::Setup(Outcome::Hooked(Ok("~/.claude/settings.json".into()))), &data);
        assert!(matches!(written, Action::Setup(Job::HookNotes)), "the notes are read anew");

        // The core lists the hook client now, the notes name it: off removes it, then its hooks.
        data.clients.push(client("h2", "Claude Code hooks", HOOK_CLIENT, Rights::hook()));
        let _ = update(&mut state, Message::Setup(Outcome::HookNotes(Ok(vec![note("h2", Some("c1"))]))), &data);
        assert!(reports(&state, &claude, &data));
        drop(view(&state, &data));
        let action = update(&mut state, Message::Report(false), &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "h2"));
        // Any `ok` could be another request's: the hooks go once a reload no longer lists the client.
        assert!(answered(&mut state, &Ok(Response::Ok)).is_none());
        assert!(loaded(&mut state, &data).is_none(), "still listed");
        data.clients.retain(|client| client.id != "h2");
        let next = loaded(&mut state, &data);
        assert!(matches!(next, Some(Action::Setup(Job::Disconnect { ref client })) if client == "h2"));
        let done = update(&mut state, Message::Setup(Outcome::Disconnected(Ok(Released::default()))), &data);
        assert!(matches!(done, Action::Setup(Job::HookNotes)));
        assert!(state.change.is_none() && state.hook_problem.is_none());
    }

    #[test]
    fn a_failed_switch_says_why_and_leaves_no_hook_client() {
        let mut data = data();
        data.clients.retain(|client| client.kind != HOOK_CLIENT);
        let mut state = opened("c1", &data, Vec::new());
        let _ = update(&mut state, Message::Report(true), &data);
        let _ = answered(&mut state, &hook_key("h2", AgentKind::ClaudeCode));
        let failed = Outcome::Hooked(Err("settings.json could not be read".into()));
        let action = update(&mut state, Message::Setup(failed), &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "h2"), "nothing uses its key");
        assert_eq!(
            state.hook_problem.as_ref().map(|problem| (problem.on, problem.why.as_str())),
            Some((true, "settings.json could not be read"))
        );
        drop(view(&state, &data));

        // The core refuses the hook client: the reason, nothing else to do.
        let mut refused = opened("c1", &data, Vec::new());
        let _ = update(&mut refused, Message::Report(true), &data);
        assert!(matches!(answered(&mut refused, &Err("busy".into())), Some(Action::None)));
        assert!(refused.change.is_none() && refused.hook_problem.is_some());

        // Taking them out fails: the page names the file to take them out of by hand.
        let mut hooked = data.clone();
        hooked.clients.push(client("h3", "Claude Code hooks", HOOK_CLIENT, Rights::hook()));
        let mut state = opened("c1", &hooked, vec![note("h3", Some("c1"))]);
        let _ = update(&mut state, Message::Report(false), &hooked);
        let _ = loaded(&mut state, &data);
        let _ = update(&mut state, Message::Setup(Outcome::Disconnected(Err("read-only".into()))), &data);
        let problem = state.hook_problem.clone().expect("the reason");
        assert_eq!((problem.on, problem.place.as_deref()), (false, Some("~/.claude/settings.json")));
        // The core refuses the removal: the next reload still lists the hook client.
        let mut kept = opened("c1", &hooked, vec![note("h3", Some("c1"))]);
        let _ = update(&mut kept, Message::Report(false), &hooked);
        assert!(answered(&mut kept, &Err("not-found".into())).is_none(), "the shell shows it too");
        let _ = loaded(&mut kept, &hooked);
        assert!(kept.change.is_none() && kept.hook_problem.is_some());

        // Held until the notes are read, and while a client is being removed.
        let mut unread = State::default();
        let _ = update(&mut unread, Message::Page(PageMessage::Open("c1".into())), &data);
        assert!(matches!(update(&mut unread, Message::Report(true), &data), Action::None));
        let _ = update(&mut unread, Message::Setup(Outcome::HookNotes(Err("client_setup.json".into()))), &data);
        assert!(unread.notes_problem.is_some(), "the list says why");
        drop(view(&unread, &data));

        // A client without hooks has no switch to flip.
        let mut cursor = data.clone();
        cursor.clients.push(client("u1", "Cursor", "cursor", Rights::none()));
        let mut state = opened("u1", &cursor, Vec::new());
        assert!(matches!(update(&mut state, Message::Report(true), &cursor), Action::None));
        drop(view(&state, &cursor));
    }

    #[test]
    fn disconnect_removes_the_configuration_entry_and_the_agent_s_hook_client() {
        let data = data();
        let mut state = opened("c1", &data, vec![note("h1", Some("c1"))]);
        let _ = update(&mut state, Message::Page(PageMessage::AskRemove), &data);
        assert!(overlay(&state, &data).is_some(), "Disconnect this client?");
        let action = update(&mut state, Message::Page(PageMessage::Remove), &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "c1"));
        let next = answered(&mut state, &Ok(Response::Ok));
        assert!(matches!(next, Some(Action::Setup(Job::Disconnect { ref client })) if client == "c1"));
        let released = Released { target: None, hook_clients: vec!["h1".into()] };
        let action = update(&mut state, Message::Setup(Outcome::Disconnected(Ok(released))), &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "h1"));
        let next = answered(&mut state, &Ok(Response::Ok));
        assert!(matches!(next, Some(Action::Setup(Job::Disconnect { ref client })) if client == "h1"));
        let last = update(&mut state, Message::Setup(Outcome::Disconnected(Ok(Released::default()))), &data);
        assert!(matches!(last, Action::Setup(Job::HookNotes)), "with none left, the notes are read anew");

        let _ = update(&mut state, Message::Setup(Outcome::Disconnected(Err("~/.cursor/mcp.json".into()))), &data);
        assert!(state.problem.is_some());
        drop(view(&state, &data));
    }

    #[test]
    fn hooks_without_an_agent_are_removed_after_asking() {
        let data = data();
        let mut state = State::default();
        let _ = update(&mut state, Message::Setup(Outcome::HookNotes(Ok(vec![note("h1", Some("gone"))]))), &data);
        drop(view(&state, &data));
        assert!(matches!(update(&mut state, Message::AskRemoveHooks("c1".into()), &data), Action::None));
        assert!(state.confirm_orphan.is_none(), "only hooks without an agent");
        let _ = update(&mut state, Message::AskRemoveHooks("h1".into()), &data);
        assert!(overlay(&state, &data).is_some() && matches!(escape(&state), Some(Message::CloseDialog)));
        let action = update(&mut state, Message::RemoveHooks, &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "h1"));
        let next = answered(&mut state, &Ok(Response::Ok));
        assert!(matches!(next, Some(Action::Setup(Job::Disconnect { ref client })) if client == "h1"));
    }

    #[test]
    fn the_server_group_switches_and_copies_the_command() {
        let data = data();
        let mut state = State::default();
        assert_eq!(header(&state, &data, SizeClass::Wide).actions.len(), 1, "Connect client");
        for on in [false, true] {
            let action = update(&mut state, Message::Allow(on), &data);
            assert!(matches!(
                action,
                Action::Send(Request::SetSetting { key: SettingKey::AllowMcp, ref value }) if *value == serde_json::Value::Bool(on)
            ));
        }
        let copy = update(&mut state, Message::CopyCommand, &data);
        assert!(matches!(copy, Action::Copy(ref command) if command.ends_with(" mcp")), "{copy:?}");
        assert!(state.copied);
        assert!(matches!(update(&mut state, Message::Connect, &data), Action::Setup(Job::Detect)));
        assert!(state.wizard.is_some() && overlay(&state, &data).is_some());
        let detected = update(&mut state, Message::Setup(crate::client_setup::demo(&Job::Detect)), &data);
        assert!(matches!(detected, Action::None));
        let _ = update(&mut state, Message::Wizard(agent_wizard::Message::Dismiss), &data);
        assert!(state.wizard.is_none(), "a click next to the first step closes it");
        drop(view(&state, &Data::default()));
    }
}
