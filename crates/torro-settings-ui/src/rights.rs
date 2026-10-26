//! The rights editor (task S.33): one view of what a client may do, for the control centre's pages
//! of keys and MCP clients and, from task S.34, the assistant window's Access. Concept:
//! `docs/settings.md` › "Der Rechte-Editor", `docs/api.md` › "Rechte".
//!
//! - **Areas:** a row per area a client of its kind can be given ([`shows_area`]) with a segmented
//!   control of the levels that make a difference there ([`levels`]), each starting in the same
//!   slot ([`segment_slot`]) so the levels line up; every other control (the switches) starts at the
//!   same edge ([`control_column`]). Where a row's name and line would not fit beside that column,
//!   each segmented control stands on a line of its own under them and the switches go to the right
//!   edge. An admin switch of its own after every area that has admin commands ([`has_admin`]);
//!   under "Mail" from "Write" on "Send on my behalf" ([`Rights::outbound`]) with a warning while it
//!   says "Always" ([`sends_freely`]); a warning under `bots` and `assistant` while they are on
//!   ([`bot_warning`]).
//! - **Commands (#231):** every area with access opens with "Show commands": a row per command the
//!   level and the admin switch allow ([`commands`]), named after [`Naming`] – the endpoint
//!   (`calendar.list_events`) on a key's page, the tool (`calendar_list_events`) on an MCP client's
//!   and in Access – with the first sentence of its catalogue description and a switch that takes
//!   the command out of [`Rights::denied`] or puts it in. The area's row says how many are off
//!   ("Calendar · 2 off", [`off_count`]). Lowering the level keeps the entries of
//!   commands it no longer allows (harmless: the level refuses them anyway); they show again when
//!   the level rises.
//! - **"Ask first"** (never, on delete, on every change) and the **selections** of calendars, task
//!   lists, GitHub repositories, inbox sources, mail accounts and the assistant's own tools (task
//!   WZ.7; "All" or the switched-on ones, [`Pick`]) for each area the rights reach; a tool that is
//!   off in the assistant's settings gets a mark in the attention colour. What they offer comes from [`Known`], which the
//!   window that shows the editor fills from its own data.
//! - **`web` only by name** (issue #305, task WEB.6): "All" tools never includes a tool that must be ticked by name
//!   ([`ToolId::needs_naming`], the web search). With "All" its row stands unticked under the choice; ticking it
//!   makes "All" a selection of every tool, itself included. "Selected" from "All" and switching the area on tick
//!   every tool but those. Ticked for a client that reads mail, it gets a warning like "Send on my behalf"
//!   ([`sends_outside`]).
//!
//! Every change edits a draft of the client's rights, which the editor shows at once
//! ([`shown`]); [`SAVE_DELAY`] after the last change (`docs/settings.md`: autosave after 500 ms) it
//! goes out as `clients.update_client` with the whole rights. The draft stays on screen until the
//! core answered ([`answered`]) and the reload after it arrived ([`loaded`]), so no control jumps
//! back for a moment. Rights no client holds yet (the assistant's first start) are changed with
//! [`change`] instead and never saved here.
//!
//! The page that shows the editor keeps its [`State`], hands its [`Message`]s to [`update`] (or
//! [`change`]), sends what comes back, and runs [`subscription`] for the clock. Only the app key may
//! send `clients.update_client`; which window may use this editor at all is a rule of the window
//! (task S.34), not of the editor.

use std::collections::BTreeSet;
use std::time::Duration;

use iced::time::Instant;
use iced::widget::{Space, column, container, responsive, row, svg, toggler};
use iced::{Alignment, Color, Element, Fill, Length, Subscription, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, palette, space};
use torro_gui::widgets::{self, color_dot};
use torro_gui::{style, typo};
use torro_inbox::{SourceInfo, SourceKind, SourceRef};
use torro_ipc::api::{
    self, Access, Area, AreaRights, CATALOGUE, Command, Confirm, HOOK_CLIENT, Level, Mcp, Operation, Outbound, Rights,
    RightsTemplate, Selection, active_level, has_command, levels, shows_area,
};
use torro_ipc::tools::{ToolId, ToolInfo};
use torro_ipc::{AccountInfo, ClientInfo, Request, Response};
use torro_pim::Collection;

use crate::Action;
use crate::parts::{field, pressable, row_frame, setting_row};

/// How long after the last change the rights are saved (`docs/settings.md`, "Speichern
/// automatisch").
pub const SAVE_DELAY: Duration = Duration::from_millis(500);

/// How often the editor looks whether a draft is due while one waits; nothing ticks otherwise.
const TICK: Duration = Duration::from_millis(100);

/// The editor's own state: the draft on its way to the core, the text in "Add repository" and the
/// areas whose commands are shown. One editor edits one client at a time; the page calls [`leave`]
/// when it shows another.
#[derive(Debug, Default)]
pub struct State {
    /// Rights changed in the editor and not yet in the core's list of clients.
    draft: Option<Draft>,
    /// What is typed into "Add repository".
    repo_input: String,
    /// The areas opened with "Show commands".
    open: BTreeSet<Area>,
}

/// Rights the editor changed, on their way to the core.
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

/// How the editor names a command: as a program calls it over the API, or as an agent sees it over
/// MCP. The same command either way; only the spelling differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Naming {
    /// The catalogue name, which is the API's endpoint: `calendar.list_events`. For keys of
    /// scripts and programs (control centre › API).
    Endpoints,
    /// The MCP tool name ([`Command::tool_name`]): `calendar_list_events`. For MCP clients
    /// (control centre › MCP) and for the assistant and its bots, whose tools carry these names.
    Tools,
}

impl Naming {
    /// `command`'s name in this naming.
    pub fn name(self, command: &Command) -> String {
        match self {
            Naming::Endpoints => command.name.to_string(),
            Naming::Tools => command.tool_name(),
        }
    }
}

/// What the selections of the editor offer: everything the window knows of each kind, filled by
/// the window that shows the editor (the control centre from its own data, an app's settings
/// window from [`crate::Data`] with [`Known::of`]).
///
/// - calendars: `calendar.list_calendars`, task lists: `tasks.list_lists` (whose GitHub filter
///   lists name the repositories offered, [`repos_in`]),
/// - mail accounts: the signed-in accounts of the kind `mail` among `accounts`
///   (`accounts.list_accounts`, [`api::signed_in_mail`]),
/// - inbox sources: `sources` (`inbox.list_sources`), the source of every one of `clients`
///   (`clients.list_clients`) and the sources of the remote accounts ([`api::present_sources`]),
/// - tools: `tools` (`tools.list_tools` with the app key, task WZ.7), each with whether it is on in
///   the assistant's settings.
///
/// The accounts also name the account a calendar or list belongs to. A list left empty offers
/// nothing of its kind; a selection still shows what it names, so it can be unticked.
#[derive(Debug, Clone, Copy, Default)]
pub struct Known<'a> {
    pub calendars: &'a [Collection],
    pub task_lists: &'a [Collection],
    pub sources: &'a [SourceInfo],
    pub clients: &'a [ClientInfo],
    pub accounts: &'a [AccountInfo],
    pub tools: &'a [ToolInfo],
}

impl<'a> Known<'a> {
    /// What the sections' data of an app's settings window knows.
    pub fn of(data: &'a crate::Data) -> Known<'a> {
        Known {
            calendars: &data.calendars,
            task_lists: &data.task_lists,
            sources: &data.sources,
            clients: &data.clients,
            accounts: &data.accounts,
            tools: &data.tools,
        }
    }
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
            // `web` must be ticked by name (`ToolId::needs_naming`): "All" does not include it.
            (Pick::Tools, true) => tr(
                "Includes tools added later, but not web search: tick it by name. Each works only while it is on in the assistant's settings.",
            ),
            (Pick::Tools, false) => {
                tr("This agent can't use tools left off. Each works only while it is on in the assistant's settings.")
            }
        }
    }
}

/// What the editor's controls report.
#[derive(Debug, Clone)]
pub enum Message {
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
    /// "Show commands" or "Hide commands" of an area: opens or closes its list of commands.
    Expand(Area),
    /// The switch of one command: its catalogue name ([`Command::name`]) and whether it is on –
    /// allowed – now. Off puts the name into [`Rights::denied`], on takes it out.
    Command(&'static str, bool),
    /// The clock while a draft waits ([`subscription`]).
    Clock(Instant),
}

/// Applies `message` to the rights of `client` – the client whose rights the editor shows – and
/// saves them [`SAVE_DELAY`] after the last change.
///
/// - Every rights control ([`change_rights`]) changes a draft of the rights shown ([`shown`]); the
///   returned action is [`Action::None`], the save comes with the clock.
/// - [`Message::Clock`] sends a waiting draft once it is due: `clients.update_client` with the
///   whole rights, as [`Action::Send`] with that one request. It needs no `client`, so a draft
///   still goes out after the page moved on.
/// - [`Message::RepoInput`] and [`Message::Expand`] only change what the editor shows.
///
/// A hook client's rights are fixed (the core narrows them to `Rights::hook` whatever is sent), and
/// without a `client` there is nothing to change: both leave everything as it is.
pub fn update<A>(state: &mut State, message: Message, client: Option<&ClientInfo>, known: &Known) -> Action<A> {
    match message {
        Message::Clock(now) => match &state.draft {
            Some(Draft { phase: Phase::Waiting(since), .. }) if now.saturating_duration_since(*since) >= SAVE_DELAY => {
                save_now(state)
            }
            _ => Action::None,
        },
        Message::RepoInput(_) | Message::Expand(_) => {
            view_only(state, message);
            Action::None
        }
        message => {
            let Some(client) = client.filter(|client| client.kind != HOOK_CLIENT) else { return Action::None };
            let mut rights = shown(state, client).clone();
            if change_rights(&mut rights, &client.kind, message, known, &mut state.repo_input) {
                let id = client.id.clone();
                state.draft = Some(Draft { id, rights, phase: Phase::Waiting(Instant::now()) });
            }
            Action::None
        }
    }
}

/// Applies `message` to `rights` that no client holds yet – what the assistant's first start offers
/// until "Turn on assistant" – for a client of `kind`. Nothing is saved: the caller keeps `rights`
/// and sends them along when it creates the client. Returns `true` when `rights` changed.
/// [`Message::RepoInput`] and [`Message::Expand`] change only what the editor shows,
/// [`Message::Clock`] nothing.
pub fn change(state: &mut State, rights: &mut Rights, kind: &str, message: Message, known: &Known) -> bool {
    match message {
        Message::Clock(_) => false,
        Message::RepoInput(_) | Message::Expand(_) => {
            view_only(state, message);
            false
        }
        message => change_rights(rights, kind, message, known, &mut state.repo_input),
    }
}

/// The messages that change only what the editor shows.
fn view_only(state: &mut State, message: Message) {
    match message {
        Message::RepoInput(text) => state.repo_input = text,
        Message::Expand(area) => toggle(&mut state.open, area),
        _ => {}
    }
}

/// Opens `area` when it is closed, closes it when it is open.
fn toggle(open: &mut BTreeSet<Area>, area: Area) {
    if !open.remove(&area) {
        open.insert(area);
    }
}

/// Applies a control of the editor to `rights` of a client of `kind`: an area's level or admin
/// switch, a command's switch, "Ask first", "Send on my behalf", "All"/"Selected" or a switch of a
/// selection, "Add" of a repository (`repo_input` is the text typed there; it is cleared once the
/// name is added).
///
/// Returns `true` when `rights` changed. Every other message, a control that changes nothing (the
/// level already set, "Selected" while already selected, a name that is no repository) and an area
/// the client's kind does not show ([`shows_area`]) leave `rights` as they are and return `false`.
/// A command's switch only takes names of the catalogue that can be switched off
/// ([`api::deniable`]), are MCP tools and belong to an area the kind shows; switching one on that is
/// not off changes nothing.
///
/// "Selected" starts with every known candidate ticked ([`candidates`]): the client keeps what it
/// reaches until the person unticks something. A level goes through [`Rights::set_level`], the rule
/// the bot editor shares (task B.5): switching an area on while nothing of it is ticked ticks
/// everything of it there is now – calendars, task lists, inbox sources, signed-in mail accounts
/// (issue #133, widened), the tools `tools.list_tools` lists (task WZ.7), switched on or not –,
/// GitHub stays empty; what is added later stays off until ticked. A level
/// leaves [`Rights::denied`] alone.
pub fn change_rights(
    rights: &mut Rights,
    kind: &str,
    message: Message,
    known: &Known,
    repo_input: &mut String,
) -> bool {
    let before = rights.clone();
    match message {
        Message::Level(area, level) if shows_area(area, kind) => {
            // Never a tool that must be ticked by name: switching `tools` on gives what "All" would.
            let present = || match Pick::ALL.into_iter().find(|pick| pick.area() == area) {
                Some(pick) => candidates(pick, known, &Selection::Only(Vec::new()))
                    .into_iter()
                    .map(|c| c.id)
                    .filter(|id| pick != Pick::Tools || !by_name_only(id))
                    .collect(),
                None => Vec::new(),
            };
            rights.set_level(area, level, present);
        }
        Message::Admin(area, admin) if shows_area(area, kind) => {
            let level = rights.area(area).level;
            rights.areas.insert(area, AreaRights { level, admin });
        }
        Message::Command(name, on) if switchable(name, kind) => {
            if on {
                rights.denied.remove(name);
            } else {
                rights.denied.insert(name.to_string());
            }
        }
        Message::Confirm(confirm) => rights.confirm = confirm,
        Message::Outbound(outbound) => rights.outbound = outbound,
        Message::All(pick, true) => *pick.of_mut(rights) = Selection::All,
        // What "All" reached stays ticked; for tools that is every one but those ticked by name.
        Message::All(pick, false) if !matches!(pick.of(rights), Selection::Only(_)) => {
            let ids = candidates(pick, known, pick.of(rights))
                .into_iter()
                .map(|candidate| candidate.id)
                .filter(|id| pick != Pick::Tools || !by_name_only(id))
                .collect();
            *pick.of_mut(rights) = Selection::Only(ids);
        }
        // Ticking `web` under "All" (where it stands unticked) selects every tool, `web` included.
        Message::Tick(Pick::Tools, id, true) if rights.tools == Selection::All && by_name_only(&id) => {
            let mut ids: Vec<String> =
                candidates(Pick::Tools, known, &Selection::All).into_iter().map(|candidate| candidate.id).collect();
            if !ids.contains(&id) {
                ids.push(id);
            }
            rights.tools = Selection::Only(ids);
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

/// Whether the command `name` has a switch for a client of `kind`: a command of the catalogue that
/// can be switched off ([`api::deniable`]), offered to agents ([`Mcp::Tool`]) and in an area the
/// kind shows.
fn switchable(name: &str, kind: &str) -> bool {
    api::deniable(name)
        && CATALOGUE
            .iter()
            .any(|command| command.name == name && command.mcp == Mcp::Tool && shows_area(command.area, kind))
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

/// Sends a waiting draft now: `clients.update_client` with the whole rights. For a page that is
/// left ("Back"), which is a good moment to save; the clock would do it a moment later.
/// [`Action::None`] while nothing waits.
pub fn save_now<A>(state: &mut State) -> Action<A> {
    match state.draft.as_mut() {
        Some(draft) if matches!(draft.phase, Phase::Waiting(_)) => {
            draft.phase = Phase::Sending;
            Action::send(Request::UpdateClient { id: draft.id.clone(), rights: draft.rights.clone() })
        }
        _ => Action::None,
    }
}

/// The answer to a request the page sent, which it hands to the editor first. While the draft is on
/// its way an `ok` means it is saved – it goes with the next reload ([`loaded`]) –, an error drops it,
/// so the editor shows what the core holds and the page shows the error. Returns whether the answer
/// was the draft's; any other answer leaves the editor alone.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> bool {
    let sending = state.draft.as_ref().is_some_and(|draft| draft.phase == Phase::Sending);
    if sending {
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
    sending
}

/// The page loaded its clients anew: a saved draft is in them now, so it goes.
pub fn loaded(state: &mut State) {
    if state.draft.as_ref().is_some_and(|draft| draft.phase == Phase::Answered) {
        state.draft = None;
    }
}

/// The client `id` is gone (removed by the page): its rights need not be saved any more.
pub fn forget(state: &mut State, id: &str) {
    if state.draft.as_ref().is_some_and(|draft| draft.id == id) {
        state.draft = None;
    }
}

/// The page shows another client's rights (or none): the text in "Add repository" goes and every
/// area closes again. A waiting draft stays and is saved as usual.
pub fn leave(state: &mut State) {
    state.repo_input.clear();
    state.open.clear();
}

/// The clock that saves a waiting draft; none while nothing waits, so the editor costs nothing at
/// rest.
pub fn subscription(state: &State) -> Subscription<Message> {
    match state.draft {
        Some(Draft { phase: Phase::Waiting(_), .. }) => iced::time::every(TICK).map(Message::Clock),
        _ => Subscription::none(),
    }
}

/// The rights the editor shows for `client`: its draft while there is one, else what the core
/// holds.
pub fn shown<'a>(state: &'a State, client: &'a ClientInfo) -> &'a Rights {
    match &state.draft {
        Some(draft) if draft.id == client.id => &draft.rights,
        _ => &client.rights,
    }
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The commands of `area` the editor lists with a switch for `rights`: every MCP tool of the area
/// ([`Mcp::Tool`]) that level, admin switch and "Send on my behalf" allow – whether it is switched
/// off or not ([`Rights::denied`]) –, in the order of the catalogue. Commands kept from agents are
/// never listed: no right opens them.
pub fn commands(area: Area, rights: &Rights) -> Vec<&'static Command> {
    let open = Rights { denied: BTreeSet::new(), ..rights.clone() };
    CATALOGUE
        .iter()
        .copied()
        .filter(|command| command.area == area && command.mcp == Mcp::Tool)
        .filter(|command| open.allows(Operation { command, access: command.access }))
        .collect()
}

/// How many of the commands [`commands`] lists for `area` are switched off. Entries for commands
/// the level no longer allows do not count: they are not shown either.
pub fn off_count(area: Area, rights: &Rights) -> usize {
    commands(area, rights).into_iter().filter(|command| rights.denied.contains(command.name)).count()
}

/// The first sentence of a command's catalogue description, for the line under its name:
/// everything up to the first full stop that ends a sentence ("e.g." does not), or all of it.
/// Still in the catalogue's Markdown; [`plain`] takes that out for people.
pub fn first_sentence(description: &str) -> &str {
    let mut from = 0;
    while let Some(found) = description[from..].find(". ") {
        let end = from + found + 1;
        let before = &description[..end];
        if !(before.ends_with("e.g.") || before.ends_with("i.e.")) {
            return before;
        }
        from = end;
    }
    description
}

/// `text` without the Markdown marks the catalogue's descriptions use for agents – backticks around
/// a name (`` `due_only` ``) and `*` for emphasis –, so the line under a command reads as plain text.
pub fn plain(text: &str) -> String {
    text.chars().filter(|c| !matches!(c, '`' | '*')).collect()
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

/// Whether the tool `id` is one a client gets only when it is ticked by name ([`ToolId::needs_naming`]: `web`), never
/// through "All". An id no tool has is not.
pub fn by_name_only(id: &str) -> bool {
    ToolId::ALL.into_iter().any(|tool| tool.name() == id && tool.needs_naming())
}

/// Whether the warning under the tools is due: the client reaches `web` (ticked by name) and may read mail, so it
/// could put text it read into search words or an address without anyone looking first.
pub fn sends_outside(rights: &Rights) -> bool {
    rights.reaches_tool(ToolId::Web) && rights.area(Area::Mail).level != Level::NoAccess
}

/// Whether the warning under "Send on my behalf" is due: the client sends without asking. Mail it
/// reads can hold instructions to it, and with "Always" nobody looks before it sends.
pub fn sends_freely(rights: &Rights) -> bool {
    can_send(rights.area(Area::Mail).level) && rights.outbound == Outbound::Free
}

/// The width of the slot every control of the editor stands in, right-aligned: as wide as the widest
/// segmented control (all four levels, "Ask first", "Send on my behalf", "All · Selected") in the
/// language shown. So every control ends on the axis of the switches and chevrons of the other rows,
/// and the page lines up in every size class without a width of its own.
pub fn segment_slot() -> f32 {
    let levels = [Level::NoAccess, Level::Read, Level::Write, Level::Full].map(widgets::level_label);
    let confirm = widgets::confirm_options().map(|(label, _)| label);
    let outbound = outbound_options().map(|(label, _)| label);
    let pick = [tr("All"), tr("Selected")];
    [&levels[..], &confirm, &outbound, &pick].into_iter().map(widgets::segmented_width).fold(0.0, f32::max)
}

/// Where the controls of the editor go on a page `width` wide: `Some(slot)`, a column
/// [`segment_slot`] wide at the right of every row, when the longest name and line of an area or
/// admin switch fit next to it in a `setting_row` – the name with room for "· 12 off"; `None` when
/// they would be cut off (a narrow window, the narrow end of the medium one), then every segmented
/// control stands on a line of its own under its name (`widgets::segment_row`) and the other
/// controls go to the right edge. Decided by what fits (`widgets::segment_column`) rather than by
/// the size class, which the page does not know; it also holds for longer translations.
pub fn control_column(width: f32) -> Option<f32> {
    let admins: Vec<(&str, String)> = Area::ALL
        .into_iter()
        .filter(|&area| area.grantable() && has_admin(area))
        .map(|area| (admin_label(area), admin_detail(area)))
        .collect();
    // The widest count the name of an area can carry: two figures.
    let names: Vec<(String, &str)> = Area::ALL
        .into_iter()
        .filter(|area| area.grantable())
        .map(|area| (area_name(area, 99), area_detail(area)))
        .collect();
    let areas = names.iter().map(|(name, detail)| (name.as_str(), Some(*detail)));
    let admins = admins.iter().map(|(label, detail)| (*label, Some(detail.as_str())));
    let sending = (tr("Send on my behalf"), Some(outbound_detail()));
    widgets::segment_column(segment_slot(), areas.chain(admins).chain([sending]), width)
}

/// `control` where the editor puts its controls: at the right of the column – where the segmented
/// controls of `widgets::segment_row` end too –, or as it is (at the right edge of its row) without
/// one ([`control_column`]).
pub fn in_column<'a, M: 'a>(control: impl Into<Element<'a, M>>, column: Option<f32>) -> Element<'a, M> {
    match column {
        Some(slot) => container(control).width(slot).align_x(Alignment::End).into(),
        None => control.into(),
    }
}

/// The area's name for people, translated: `Calendar`, `API and MCP`. The assistant's tool lines
/// name areas with it too (issue #133).
pub fn area_label(area: Area) -> &'static str {
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
        Area::Clients => tr("API and MCP"),
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

/// An area's name in its row with how many of its commands are switched off while some are
/// ("Calendar · 2 off"), as [`control_column`] reckons its width. On the name, not on the line below
/// it: the names are short, so the count fits beside the column of controls in every language, where
/// the longest line would be cut. The row shows the count quieter than the name ([`off_note`]).
pub fn area_name(area: Area, off: usize) -> String {
    match off_note(off) {
        None => area_label(area).to_string(),
        Some(note) => format!("{} {note}", area_label(area)),
    }
}

/// What follows an area's name while some of its commands are switched off: "· 2 off", shown in
/// `META` and `text_3` (`widgets::segment_row_noted`); `None` while none are.
pub fn off_note(off: usize) -> Option<String> {
    (off > 0).then(|| format!("· {}", trf("{n} off", &[("n", &off)])))
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

/// The name of a template of rights, translated.
pub fn template_label(template: RightsTemplate) -> &'static str {
    match template {
        RightsTemplate::ReadOnly => tr("Read only"),
        RightsTemplate::ReadAndWrite => tr("Read and write"),
        RightsTemplate::FullAccess => tr("Full access"),
    }
}

/// A client's rights in a few words for a list: the template they equal ("Read and write"),
/// else "Custom: Calendar, Tasks, Inbox" (at most three areas, then "+2"); "No access" without
/// any area; for a hook what it can do at all. Rights with commands switched off are custom.
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

/// What a selection offers: everything `known` holds of its kind, then what `selection` names that
/// is no longer known (marked, so it can be unticked).
pub fn candidates(pick: Pick, known: &Known, selection: &Selection) -> Vec<Candidate> {
    let mut offered = match pick {
        Pick::Calendars => known.calendars.iter().map(|collection| collection_candidate(collection, known)).collect(),
        Pick::TaskLists => known.task_lists.iter().map(|collection| collection_candidate(collection, known)).collect(),
        Pick::GithubRepos => repo_candidates(known, selection),
        Pick::InboxSources => source_candidates(known, selection),
        Pick::MailAccounts => mail_candidates(known),
        Pick::Tools => tool_candidates(known),
    };
    // Repositories need no such pass: their candidates are made from the selection.
    if let (Selection::Only(ids), false) = (selection, pick == Pick::GithubRepos) {
        let gone: Vec<Candidate> = ids
            .iter()
            .filter(|id| !offered.iter().any(|candidate: &Candidate| candidate.id == **id))
            .map(|id| Candidate::plain(id.clone(), id.clone(), Some(tr("No longer available").to_string())))
            .collect();
        offered.extend(gone);
    }
    offered
}

/// A calendar or list: its colour, and the account it belongs to (a GitHub filter list says so).
fn collection_candidate(collection: &Collection, known: &Known) -> Candidate {
    let account = known
        .accounts
        .iter()
        .find(|account| account.collections.iter().any(|held| held.id == collection.id))
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
/// spells it), in alphabetical order. There is no list of all repositories; others are typed in
/// ("Add repository").
fn repo_candidates(known: &Known, selection: &Selection) -> Vec<Candidate> {
    // The selection's spelling first, so its names show ticked.
    let mut names: Vec<String> = match selection {
        Selection::Only(selected) => selected.clone(),
        Selection::All => Vec::new(),
    };
    for name in repos_in(known.task_lists) {
        if !names.iter().any(|held| held.eq_ignore_ascii_case(&name)) {
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
        if !names.iter().any(|held| held.eq_ignore_ascii_case(name)) {
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

/// The inbox sources a client can be allowed: the sources with entries or muted
/// (`inbox.list_sources`), the source of every connected client (where its reports would land,
/// [`api::post_source`]), every mail account (its new mail, task E4.1: the source's key is the
/// account's id), the inbox sources of every GitHub, Nextcloud and ntfy account (task E5.14:
/// GitHub, Nextcloud, Talk, ntfy) and those `selection` names; each once, in the order of
/// [`SourceKind`] (agents, scripts, mail, GitHub, Nextcloud, Talk, ntfy). Each remote source says its
/// account below its name; GitHub adds that its entries also need read access to GitHub and the
/// repository, which the core checks besides the source (task E5.2).
/// Never the suite: its entries (approvals, account problems) are only for the person, the core
/// shows them to no client whatever the selection says (task E1.2c). A selection that still names
/// it shows it as no longer available ([`candidates`]), so it can be unticked.
fn source_candidates(known: &Known, selection: &Selection) -> Vec<Candidate> {
    let mut sources = api::present_sources(known.sources, known.clients, known.accounts);
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

/// The mail accounts a client can be allowed (`accounts.list_accounts`, kind `mail`): the id is
/// what the core checks a call against ([`Rights::reaches_mail_account`]), the address is what the
/// person knows it by. A sign-in that is still running has no id and no mail yet.
fn mail_candidates(known: &Known) -> Vec<Candidate> {
    api::signed_in_mail(known.accounts)
        .map(|account| Candidate::plain(account.id.clone(), account.label.clone(), None))
        .collect()
}

/// The assistant's own tools (`tools.list_tools`, task WZ.7): the id is what the core checks a
/// command against ([`Rights::reaches_tool`]), the name what people call it ("PDF"). Below it what
/// the tool does, or, while it is off in the assistant's settings, that it is: granted, it still
/// does nothing until the person switches it on there.
fn tool_candidates(known: &Known) -> Vec<Candidate> {
    known
        .tools
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

/// What a selection shows under "All": the tools that must be ticked by name ([`by_name_only`]), which "All" does
/// not include, each to be shown unticked; nothing for every other selection, whose "All" includes everything.
pub fn named_under_all(pick: Pick, known: &Known) -> Vec<Candidate> {
    match pick {
        Pick::Tools => tool_candidates(known).into_iter().filter(|candidate| by_name_only(&candidate.id)).collect(),
        _ => Vec::new(),
    }
}

/// Whether the tool `id` is known and switched off in the assistant's settings
/// (`tools.list_tools`): granted, it still does nothing.
fn tool_off(known: &Known, id: &str) -> bool {
    known.tools.iter().any(|tool| tool.id.name() == id && !tool.enabled)
}

// ----- Views --------------------------------------------------------------------------------

/// The editor for `rights` of a client of `kind`, in the width it has ([`control_column`]): the
/// groups of [`groups`] one under the other. For a page with nothing else that has to line up with
/// the editor's column; a page that has (a key's row) lays out [`groups`] itself.
pub fn view<'a>(
    state: &'a State,
    rights: &'a Rights,
    kind: &'a str,
    known: Known<'a>,
    naming: Naming,
    title: &'a str,
) -> Element<'a, Message> {
    responsive(move |size| {
        let groups = groups(state, rights, kind, &known, naming, title, control_column(size.width));
        column(groups).spacing(torro_gui::layout::section_gap()).into()
    })
    .height(Length::Shrink)
    .into()
}

/// The groups of the editor for `rights` of a client of `kind`: the access group titled `title`
/// (levels, admin switches, sending, the commands of every open area), "Ask first", and the
/// selection of each area the rights reach (calendars, task lists, repositories, inbox sources,
/// mail accounts, tools), with the controls in `column` ([`control_column`]). Commands are named after
/// `naming`. Every control sends a [`Message`]; the page hands it to [`update`] or [`change`].
pub fn groups<'a>(
    state: &'a State,
    rights: &Rights,
    kind: &str,
    known: &Known,
    naming: Naming,
    title: &'a str,
    column: Option<f32>,
) -> Vec<Element<'a, Message>> {
    let mut groups =
        vec![access_group(&state.open, rights, column, kind, naming, title), confirm_group(rights.confirm, column)];
    for pick in Pick::ALL.into_iter().filter(|pick| rights.area(pick.area()).level != Level::NoAccess) {
        groups.push(selection_group(&state.repo_input, pick, rights, known, column));
    }
    groups
}

/// The level of every area a client of `kind` can be given ([`shows_area`]), each followed by its
/// admin switch where it has one and, where the area grants anything, "Show commands" with the
/// commands of the areas in `open`. The admin switch is a row of its own, not indented: admin
/// commands are allowed per area whatever the level (`docs/api.md`, "Rechte"), so its line names the
/// area instead.
fn access_group<'a>(
    open: &BTreeSet<Area>,
    rights: &Rights,
    column: Option<f32>,
    kind: &str,
    naming: Naming,
    title: &'a str,
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
        let note = off_note(off_count(area, rights));
        rows.push(widgets::segment_row_noted(
            area_label(area),
            note,
            Some(area_detail(area).to_string()),
            control,
            column,
        ));
        rows.extend(bot_warning(area, rights, kind).map(warning_row));
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
        let listed = commands(area, rights);
        if listed.is_empty() {
            continue;
        }
        let shown = open.contains(&area);
        rows.push(disclosure_row(area, shown));
        if shown {
            rows.extend(listed.into_iter().map(|command| command_row(command, rights, naming, column)));
        }
    }
    let footer = tr("Write includes reading, Full also deletes and overwrites. Commands switched off stay off.");
    widgets::group(title, rows, Some(footer.to_string()))
}

/// "Show commands" under an area, or "Hide commands" while its commands are shown; the whole row
/// is the button. Its chevron stands where the names of the commands below start ([`indent`] and
/// the gap of a `setting_row`'s mark), so the row and the commands it opens share one axis.
fn disclosure_row<'a>(area: Area, shown: bool) -> Element<'a, Message> {
    let (label, which) = match shown {
        true => (tr("Hide commands"), Icon::ChevronUp),
        false => (tr("Show commands"), Icon::ChevronDown),
    };
    let mark = icon::icon(which, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let words = typo::text(typo::body(), label).style(theme::text_2);
    let label = row![mark, words].spacing(space::s()).align_y(Alignment::Center);
    let content = row![indent(), label].spacing(space::m()).align_y(Alignment::Center);
    pressable(row_frame(content, false), Some(Message::Expand(area)))
}

/// One command under its area: its name after `naming`, the first sentence of its description, and
/// a switch that is on while the command is not in [`Rights::denied`]; the whole row toggles it.
/// Indented, so the commands read as part of the area above them.
fn command_row<'a>(
    command: &'static Command,
    rights: &Rights,
    naming: Naming,
    column: Option<f32>,
) -> Element<'a, Message> {
    let on = !rights.denied.contains(command.name);
    let name = command.name;
    let switch =
        toggler(on).size(style::CHECK_SIZE).style(style::toggler).on_toggle(move |on| Message::Command(name, on));
    let detail = Some(plain(first_sentence(command.description)));
    let row = setting_row(Some(indent()), naming.name(command), detail, in_column(switch, column));
    pressable(row, Some(Message::Command(name, !on)))
}

/// The room before the commands of an area, which sets them off from the area's own rows.
fn indent<'a>() -> Element<'a, Message> {
    Space::new().width(space::l()).into()
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

/// The line under "Send on my behalf": what sending means, so "Never" reads as a real choice.
fn outbound_detail() -> &'static str {
    tr("Messages leave your computer and can't be taken back")
}

/// "Send on my behalf" under the mail area, and while it says "Always" the warning in a row of its
/// own.
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
    known: &Known,
    column: Option<f32>,
) -> Element<'a, Message> {
    let selection = pick.of(rights);
    let all = matches!(selection, Selection::All);
    let switch = widgets::segmented(
        [(tr("All"), Message::All(pick, true)), (tr("Selected"), Message::All(pick, false))],
        usize::from(!all),
    );
    let mut rows = vec![widgets::segment_row(tr("Access to"), None, switch, column)];
    if all {
        let named = named_under_all(pick, known).into_iter();
        rows.extend(named.map(|candidate| candidate_row(pick, candidate, false, None, column)));
    }
    if !all {
        let offered = candidates(pick, known, selection);
        if offered.is_empty() && pick != Pick::GithubRepos {
            rows.push(row_frame(
                typo::text(typo::body(), tr("Nothing to choose from yet.")).style(theme::text_3),
                false,
            ));
        }
        // A tool switched off in the assistant's settings gets a mark in the attention colour; the
        // other tools keep its room, so the names line up.
        let any_off = pick == Pick::Tools && offered.iter().any(|candidate| tool_off(known, &candidate.id));
        rows.extend(offered.into_iter().map(|candidate| {
            let ticked = selection.contains(&candidate.id);
            let off = any_off.then(|| tool_off(known, &candidate.id));
            candidate_row(pick, candidate, ticked, off, column)
        }));
        if pick == Pick::GithubRepos {
            rows.push(add_repo_row(repo_input));
        }
    }
    if pick == Pick::Tools && sends_outside(rights) {
        rows.push(warning_row(tr(
            "This agent can then put text it read into search words and web addresses. Only the assistant asks first, once in each conversation.",
        )));
    }
    widgets::group(pick.title(), rows, Some(pick.footer(all).to_string()))
}

/// A candidate as on the Accounts page: a calendar's or list's colour only as a dot before the
/// name (a calendar the core no longer knows keeps the dot's room, so the names line up), a
/// neutral switch at the right; the whole row toggles it. `off` marks a tool that is off in the
/// assistant's settings (`Some(true)`, an alert in the attention colour) or keeps that mark's room
/// (`Some(false)`); `None` has no such mark.
fn candidate_row<'a>(
    pick: Pick,
    candidate: Candidate,
    ticked: bool,
    off: Option<bool>,
    column: Option<f32>,
) -> Element<'a, Message> {
    let Candidate { id, label, detail, color } = candidate;
    let switch_id = id.clone();
    let switch = toggler(ticked)
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle(move |on| Message::Tick(pick, switch_id.clone(), on));
    let dot = match (pick, color) {
        (_, Some(color)) => Some(color_dot(color, space::s())),
        (Pick::Tools, None) if off == Some(true) => Some(
            icon::icon(Icon::CircleAlert, icon::SIZE)
                .style(|t: &Theme, _| svg::Style { color: Some(palette(t).attention_text) })
                .into(),
        ),
        (Pick::Tools, None) if off == Some(false) => Some(Space::new().width(icon::SIZE).into()),
        (Pick::Calendars | Pick::TaskLists, None) => Some(Space::new().width(space::s()).into()),
        (Pick::GithubRepos | Pick::InboxSources | Pick::MailAccounts | Pick::Tools, None) => None,
    };
    let row = setting_row(dot, label, detail, in_column(switch, column));
    pressable(row, Some(Message::Tick(pick, id, !ticked)))
}

/// "owner/name or owner" and "Add"; Enter adds too. Disabled until the text is a valid name.
fn add_repo_row(input: &str) -> Element<'_, Message> {
    let add = valid_repo(input).map(|_| Message::AddRepo);
    let field = field(tr("owner/name or owner"), input).on_input(Message::RepoInput).on_submit_maybe(add.clone());
    let button = widgets::text_button(Some(Icon::Plus), tr("Add"), style::button::secondary, add);
    row_frame(row![field, button].spacing(space::s()).align_y(Alignment::Center), false)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use torro_inbox::AgentKind;
    use torro_ipc::api::{ASSISTANT_CLIENT, BOT_CLIENT};
    use torro_pim::model::GitHubFilter;
    use torro_pim::{AccountState, CollectionKind};

    use super::*;

    /// What [`Known`] borrows from in the tests.
    struct Fixture {
        clients: Vec<ClientInfo>,
        calendars: Vec<Collection>,
        task_lists: Vec<Collection>,
        sources: Vec<SourceInfo>,
        accounts: Vec<AccountInfo>,
        tools: Vec<ToolInfo>,
    }

    impl Fixture {
        fn known(&self) -> Known<'_> {
            Known {
                calendars: &self.calendars,
                task_lists: &self.task_lists,
                sources: &self.sources,
                clients: &self.clients,
                accounts: &self.accounts,
                tools: &self.tools,
            }
        }
    }

    /// The tool PDF as `tools.list_tools` lists it for the app key, switched on or not.
    fn pdf(enabled: bool) -> ToolInfo {
        ToolInfo { id: ToolId::Pdf, name: "PDF".into(), description: "Makes PDFs.".into(), enabled, allowed: true }
    }

    /// The tool web as `tools.list_tools` lists it for the app key.
    fn web(enabled: bool) -> ToolInfo {
        ToolInfo {
            id: ToolId::Web,
            name: "Web".into(),
            description: "Searches the web.".into(),
            enabled,
            allowed: true,
        }
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

    fn fixture() -> Fixture {
        Fixture {
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
        }
    }

    /// Hands `message` to the editor for the client `id` of the fixture.
    fn send(state: &mut State, fixture: &Fixture, id: &str, message: Message) -> Action<()> {
        let client = fixture.clients.iter().find(|client| client.id == id);
        update(state, message, client, &fixture.known())
    }

    /// Runs the clock past the delay and returns the request it sends.
    fn saved(state: &mut State, fixture: &Fixture) -> (String, Rights) {
        let later = Instant::now() + SAVE_DELAY + TICK;
        match update::<()>(state, Message::Clock(later), None, &fixture.known()) {
            Action::Send(mut requests) => match requests.pop() {
                Some(Request::UpdateClient { id, rights }) if requests.is_empty() => (id, rights),
                other => panic!("expected one clients.update_client, got {other:?}"),
            },
            other => panic!("expected clients.update_client, got {other:?}"),
        }
    }

    #[test]
    fn a_change_is_saved_once_after_the_delay_with_the_whole_rights() {
        let fixture = fixture();
        let mut state = State::default();
        assert_eq!(send(&mut state, &fixture, "c1", Message::Level(Area::Calendar, Level::Read)), Action::None);
        assert_eq!(send(&mut state, &fixture, "c1", Message::Confirm(Confirm::Never)), Action::None);
        assert_eq!(update::<()>(&mut state, Message::Clock(Instant::now()), None, &fixture.known()), Action::None);
        let (id, rights) = saved(&mut state, &fixture);
        assert_eq!(id, "c1");
        assert_eq!(rights.area(Area::Calendar), AreaRights { level: Level::Read, admin: false });
        assert_eq!(rights.confirm, Confirm::Never, "both changes in one request");
        assert_eq!(rights.area(Area::Tasks).level, Level::Full, "the rest unchanged");
        let later = Instant::now() + SAVE_DELAY + TICK;
        assert_eq!(update::<()>(&mut state, Message::Clock(later), None, &fixture.known()), Action::None, "sent once");
    }

    /// What the saved rights must show after a row of messages.
    type Check = fn(&Rights) -> bool;

    #[test]
    fn every_control_ends_in_update_client() {
        let fixture = fixture();
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
            // #231: a command's switch, off and on again.
            (vec![Message::Command("calendar.delete_event", false)], |r| r.denied.iter().eq(["calendar.delete_event"])),
            (
                vec![Message::Command("calendar.delete_event", false), Message::Command("calendar.delete_event", true)],
                |r| r.denied.is_empty() && r.area(Area::Calendar).level == Level::Full,
            ),
        ];
        for (messages, check) in cases {
            let mut state = State::default();
            let text = format!("{messages:?}");
            for message in messages {
                assert_eq!(send(&mut state, &fixture, "c1", message), Action::None, "{text}: waits");
            }
            let (id, rights) = saved(&mut state, &fixture);
            assert_eq!(id, "c1", "{text}");
            assert!(check(&rights), "{text}: {rights:?}");
        }
    }

    /// #231: an open area lists only the commands its level and admin switch allow, MCP tools only.
    #[test]
    fn an_open_area_lists_only_the_commands_the_level_allows() {
        let mut rights = Rights::none();
        assert!(commands(Area::Calendar, &rights).is_empty(), "no access: nothing to list");
        rights.areas.insert(Area::Calendar, AreaRights { level: Level::Read, admin: false });
        let read: Vec<&str> = commands(Area::Calendar, &rights).into_iter().map(|c| c.name).collect();
        assert!(read.contains(&"calendar.list_events") && read.contains(&"calendar.list_calendars"));
        assert!(!read.contains(&"calendar.save_event") && !read.contains(&"calendar.delete_event"), "{read:?}");
        rights.areas.insert(Area::Calendar, AreaRights { level: Level::Full, admin: false });
        let full: Vec<&str> = commands(Area::Calendar, &rights).into_iter().map(|c| c.name).collect();
        assert!(full.contains(&"calendar.save_event") && full.contains(&"calendar.delete_event"));
        for area in Area::ALL {
            let open = Rights {
                areas: BTreeMap::from([(area, AreaRights { level: Level::Full, admin: true })]),
                ..rights.clone()
            };
            for command in commands(area, &open) {
                assert_eq!((command.area, command.mcp), (area, Mcp::Tool), "{}", command.name);
            }
        }

        // Admin commands only with the admin switch.
        let mut accounts = Rights::none();
        accounts.areas.insert(Area::Accounts, AreaRights { level: Level::Read, admin: false });
        let admin = |rights: &Rights| commands(Area::Accounts, rights).iter().any(|c| c.access == Access::Admin);
        assert!(!admin(&accounts));
        accounts.areas.insert(Area::Accounts, AreaRights { level: Level::Read, admin: true });
        assert!(admin(&accounts));

        // A command switched off is still listed (with its switch off), and counted.
        rights.denied.insert("calendar.delete_event".into());
        assert!(commands(Area::Calendar, &rights).iter().any(|c| c.name == "calendar.delete_event"));
        assert_eq!(off_count(Area::Calendar, &rights), 1);
        // Lowered to Read, the entry stays but is neither listed nor counted; it shows again on Full.
        rights.areas.insert(Area::Calendar, AreaRights { level: Level::Read, admin: false });
        assert_eq!(off_count(Area::Calendar, &rights), 0);
        assert!(rights.denied.contains("calendar.delete_event"));
    }

    /// #231: lowering the level keeps the commands switched off, raising it shows them again.
    #[test]
    fn a_lower_level_keeps_the_switched_off_commands() {
        let fixture = fixture();
        let mut rights = RightsTemplate::FullAccess.rights();
        let mut input = String::new();
        let mut apply =
            |rights: &mut Rights, message| change_rights(rights, "codex", message, &fixture.known(), &mut input);
        assert!(apply(&mut rights, Message::Command("calendar.delete_event", false)));
        assert!(apply(&mut rights, Message::Level(Area::Calendar, Level::Read)));
        assert!(rights.denied.contains("calendar.delete_event"), "harmless, and kept");
        assert!(apply(&mut rights, Message::Level(Area::Calendar, Level::Full)));
        assert_eq!(off_count(Area::Calendar, &rights), 1, "back with the level");
        assert!(!apply(&mut rights, Message::Command("calendar.save_event", true)), "on already");
    }

    /// #231: only commands of the catalogue that can be switched off, and only in areas the kind
    /// shows, get a switch.
    #[test]
    fn only_real_commands_of_shown_areas_can_be_switched_off() {
        let fixture = fixture();
        let mut input = String::new();
        let mut apply = |rights: &mut Rights, kind: &str, name: &'static str| {
            change_rights(rights, kind, Message::Command(name, false), &fixture.known(), &mut input)
        };
        let mut rights = RightsTemplate::FullAccess.rights();
        assert!(!apply(&mut rights, "codex", "system.ping"), "ping cannot be switched off");
        assert!(!apply(&mut rights, "codex", "calendar.gone"), "unknown to the catalogue");
        assert!(!apply(&mut rights, "codex", "clients.update_client"), "kept from agents");
        assert!(!apply(&mut rights, BOT_CLIENT, "bots.list_bots"), "a bot has no row for bots");
        assert!(apply(&mut rights, "codex", "bots.list_bots"));
        assert!(rights.denied.iter().eq(["bots.list_bots"]));
    }

    #[test]
    fn commands_are_named_after_the_naming() {
        let command = CATALOGUE.iter().find(|c| c.name == "calendar.list_events").expect("in the catalogue");
        assert_eq!(Naming::Endpoints.name(command), "calendar.list_events");
        assert_eq!(Naming::Tools.name(command), "calendar_list_events");
        assert_eq!(first_sentence("Deletes the event. It cannot be restored."), "Deletes the event.");
        assert_eq!(first_sentence("Pauses, e.g. during a talk. Then more."), "Pauses, e.g. during a talk.");
        assert_eq!(first_sentence("No full stop"), "No full stop");
        // #231: the line under a command shows no Markdown of the catalogue.
        assert_eq!(
            plain("Lists them: with `due_only` *only* the due ones."),
            "Lists them: with due_only only the due ones."
        );
        for command in CATALOGUE {
            let line = plain(first_sentence(command.description));
            assert!(!line.is_empty(), "{}", command.name);
            assert!(!line.contains(['`', '*']), "{}", command.name);
        }
    }

    /// #231: the area's row says how many of its commands are off; "Show commands" opens the list.
    #[test]
    fn an_area_opens_and_says_how_many_commands_are_off() {
        let fixture = fixture();
        assert_eq!(area_name(Area::Calendar, 0), tr("Calendar"));
        let two = trf("{n} off", &[("n", &2)]);
        assert_eq!(area_name(Area::Calendar, 2), format!("{} · {two}", tr("Calendar")));
        assert_eq!(off_note(0), None);
        assert_eq!(off_note(2), Some(format!("· {two}")));

        let mut state = State::default();
        assert_eq!(send(&mut state, &fixture, "c1", Message::Expand(Area::Calendar)), Action::None);
        assert!(state.open.contains(&Area::Calendar));
        assert_eq!(subscription(&state).units(), 0, "opening is no change of rights");
        let client = &fixture.clients[0];
        for naming in [Naming::Endpoints, Naming::Tools] {
            drop(groups(&state, shown(&state, client), &client.kind, &fixture.known(), naming, "Access", None));
        }
        let _ = send(&mut state, &fixture, "c1", Message::Expand(Area::Calendar));
        assert!(state.open.is_empty(), "and closes again");
        let _ = send(&mut state, &fixture, "c1", Message::Expand(Area::Tasks));
        leave(&mut state);
        assert!(state.open.is_empty(), "another client starts closed");
    }

    /// Task B.5: the rule of issue #133 holds for every selection. Switching an area on while
    /// nothing of it is ticked ticks what there is now (never "All"); GitHub stays empty, a choice
    /// stays.
    #[test]
    fn switching_an_area_on_ticks_what_there_is_now() {
        let fixture = fixture();
        let mut repo_input = String::new();
        let mut rights = Rights::none();
        let mut level = |rights: &mut Rights, area: Area, level: Level| {
            change_rights(rights, BOT_CLIENT, Message::Level(area, level), &fixture.known(), &mut repo_input)
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

    /// Task WZ.7: a client without tools that gets the area has every tool there is ticked
    /// (`Only(["pdf"])`, never "All"), and unticking it again goes out as `clients.update_client`
    /// with the whole rights.
    #[test]
    fn switching_tools_on_ticks_the_tools_and_unticking_saves_the_rights() {
        let mut fixture = fixture();
        let rights = &mut fixture.clients[0].rights;
        rights.areas.insert(Area::Tools, AreaRights { level: Level::NoAccess, admin: false });
        rights.tools = Selection::Only(Vec::new());
        let mut state = State::default();
        assert_eq!(send(&mut state, &fixture, "c1", Message::Level(Area::Tools, Level::Write)), Action::None);
        let (id, rights) = saved(&mut state, &fixture);
        assert_eq!(id, "c1");
        assert_eq!(rights.area(Area::Tools).level, Level::Write);
        assert_eq!(rights.tools, Selection::Only(vec!["pdf".into()]));

        // The core took it and the reload shows it: unticking PDF is the next request.
        fixture.clients[0].rights = rights;
        let mut state = State::default();
        assert_eq!(send(&mut state, &fixture, "c1", Message::Tick(Pick::Tools, "pdf".into(), false)), Action::None);
        let (id, rights) = saved(&mut state, &fixture);
        assert_eq!(id, "c1");
        assert_eq!(rights.tools, Selection::Only(Vec::new()));
        assert_eq!(rights.area(Area::Tools).level, Level::Write, "the level stays");

        // On the assistant's first start the choice gets the same, nothing is sent.
        let mut choice = Rights::assistant();
        let message = Message::Level(Area::Tools, Level::Write);
        assert!(change(&mut State::default(), &mut choice, ASSISTANT_CLIENT, message, &fixture.known()));
        assert_eq!(choice.tools, Selection::Only(vec!["pdf".into()]));
    }

    /// Task WZ.7: a tool is offered by its id and name; below it what it does, or that it is off in
    /// the assistant's settings, since granted it still does nothing there.
    #[test]
    fn tools_are_offered_with_what_they_do_or_that_they_are_off() {
        let mut fixture = fixture();
        let offered = candidates(Pick::Tools, &fixture.known(), &Selection::All);
        assert_eq!(offered.iter().map(|c| (c.id.as_str(), c.label.as_str())).collect::<Vec<_>>(), [("pdf", "PDF")]);
        assert_eq!(offered[0].detail.as_deref(), Some(tr("Off in the assistant's settings")));
        assert!(offered[0].color.is_none());
        assert!(tool_off(&fixture.known(), "pdf"), "so its row gets the attention mark");
        assert!(!tool_off(&fixture.known(), "chart"), "a tool the core does not know is not off, it is gone");
        fixture.tools = vec![pdf(true)];
        assert_eq!(
            candidates(Pick::Tools, &fixture.known(), &Selection::All)[0].detail.as_deref(),
            Some("Makes PDFs.")
        );
        assert!(!tool_off(&fixture.known(), "pdf"));
        // A tool the core no longer knows stays in the list to be unticked.
        let stale = Selection::Only(vec!["pdf".into(), "chart".into()]);
        let all = candidates(Pick::Tools, &fixture.known(), &stale);
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].detail.as_deref(), Some(tr("No longer available")));
        assert_eq!(Pick::Tools.area(), Area::Tools);
        // Without an answer of `tools.list_tools` there is nothing to choose from.
        assert!(candidates(Pick::Tools, &Known::default(), &Selection::All).is_empty());
    }

    /// Task WEB.6: "All" tools shows `web` unticked and does not reach it; ticking it gives every tool with `web`;
    /// "Selected" from "All" and switching the area on never tick it; unticked it goes again.
    #[test]
    fn web_is_only_ever_ticked_by_name() {
        let mut fixture = fixture();
        fixture.tools = vec![pdf(true), web(false)];
        let known = fixture.known();
        let mut repo = String::new();
        let mut change =
            |rights: &mut Rights, message: Message| change_rights(rights, ASSISTANT_CLIENT, message, &known, &mut repo);
        let ids = |selection: &Selection| match selection {
            Selection::Only(ids) => ids.clone(),
            Selection::All => panic!("expected a selection, got All"),
        };

        let mut rights = RightsTemplate::ReadOnly.rights();
        assert_eq!(rights.tools, Selection::All, "the templates give all tools");
        assert!(rights.reaches_tool(ToolId::Pdf) && !rights.reaches_tool(ToolId::Web));
        let shown: Vec<String> = named_under_all(Pick::Tools, &known).into_iter().map(|c| c.id).collect();
        assert_eq!(shown, ["web"], "under All only web stands, unticked");
        assert!(named_under_all(Pick::Calendars, &known).is_empty());
        assert!(by_name_only("web") && !by_name_only("pdf") && !by_name_only("chart"));

        assert!(change(&mut rights, Message::Tick(Pick::Tools, "web".into(), true)));
        assert_eq!(ids(&rights.tools), ["pdf", "web"], "ticking web selects every tool with it");
        assert!(rights.reaches_tool(ToolId::Web));
        assert!(change(&mut rights, Message::Tick(Pick::Tools, "web".into(), false)));
        assert_eq!(ids(&rights.tools), ["pdf"]);

        // "Selected" from "All" keeps what All reached: never web.
        let mut rights = RightsTemplate::ReadOnly.rights();
        assert!(change(&mut rights, Message::All(Pick::Tools, false)));
        assert_eq!(ids(&rights.tools), ["pdf"]);
        // "All" again leaves web out as before; a tick off under All changes nothing.
        assert!(change(&mut rights, Message::All(Pick::Tools, true)));
        assert!(!change(&mut rights, Message::Tick(Pick::Tools, "pdf".into(), false)));

        // Switching the area on ticks every tool there is but web.
        let mut rights = Rights::none();
        assert!(change(&mut rights, Message::Level(Area::Tools, Level::Read)));
        assert_eq!(ids(&rights.tools), ["pdf"]);
    }

    /// Task WEB.6: `web` ticked for a client that reads mail gets the warning; without mail or under "All" none.
    #[test]
    fn web_for_a_client_that_reads_mail_is_warned_about() {
        let mut rights = RightsTemplate::ReadOnly.rights();
        assert!(rights.area(Area::Mail).level >= Level::Read);
        assert!(!sends_outside(&rights), "All does not reach web");
        rights.tools = Selection::Only(vec!["web".into()]);
        assert!(sends_outside(&rights));
        rights.areas.insert(Area::Mail, AreaRights { level: Level::NoAccess, admin: false });
        assert!(!sends_outside(&rights), "no mail, nothing read to send");
        let mut fixture = fixture();
        fixture.tools = vec![pdf(true), web(true)];
        for tools in [Selection::All, Selection::Only(vec!["web".into()])] {
            let mut rights = RightsTemplate::ReadOnly.rights();
            rights.areas.insert(Area::Tools, AreaRights { level: Level::Read, admin: false });
            rights.tools = tools;
            drop(selection_group("", Pick::Tools, &rights, &fixture.known(), None));
        }
    }

    /// Task S.30: the data rule left the rights; the access group of the assistant with mail builds
    /// without it (it stands under AI now).
    #[test]
    fn the_rights_have_no_data_rule_any_more() {
        let mut rights = Rights::assistant();
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
        let open = BTreeSet::from([Area::Mail]);
        for kind in [ASSISTANT_CLIENT, BOT_CLIENT, "claude-code"] {
            drop(access_group(&open, &rights, None, kind, Naming::Tools, "Access"));
        }
    }

    /// Rights no client holds yet change without a draft, and nothing is saved.
    #[test]
    fn rights_without_a_client_change_but_are_never_saved() {
        let fixture = fixture();
        let mut state = State::default();
        let mut choice = Rights::assistant();
        assert!(change(
            &mut state,
            &mut choice,
            ASSISTANT_CLIENT,
            Message::Level(Area::Tasks, Level::Write),
            &fixture.known()
        ));
        assert_eq!(choice.area(Area::Tasks).level, Level::Write);
        assert!(!change(&mut state, &mut choice, ASSISTANT_CLIENT, Message::Expand(Area::Tasks), &fixture.known()));
        assert!(state.open.contains(&Area::Tasks));
        assert!(change(
            &mut state,
            &mut choice,
            ASSISTANT_CLIENT,
            Message::Command("tasks.list_lists", false),
            &fixture.known()
        ));
        assert_eq!(subscription(&state).units(), 0, "no draft");
        assert_eq!(save_now::<()>(&mut state), Action::None);
    }

    #[test]
    fn mail_accounts_are_the_accounts_of_kind_mail_that_are_signed_in() {
        let fixture = fixture();
        let known = fixture.known();
        let ids = |selection: &Selection| -> Vec<String> {
            candidates(Pick::MailAccounts, &known, selection).into_iter().map(|c| c.id).collect()
        };
        assert_eq!(ids(&Selection::All), ["mail:anna@mailbox.org", "mail:anna@work.example.net"]);
        let offered = candidates(Pick::MailAccounts, &known, &Selection::All);
        assert_eq!(offered[0].label, "anna@mailbox.org", "named by the address");
        assert!(offered[0].color.is_none());
        // An account that was removed stays in the list to be unticked.
        let stale = Selection::Only(vec!["mail:gone@example.org".into()]);
        let all = candidates(Pick::MailAccounts, &known, &stale);
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
        let fixture = fixture();
        let mut state = State::default();
        let _ = send(&mut state, &fixture, "c1", Message::Outbound(Outbound::Free));
        assert_eq!(shown(&state, &fixture.clients[0]).outbound, Outbound::Free, "shown at once");
        let (_, rights) = saved(&mut state, &fixture);
        assert_eq!(rights.outbound, Outbound::Free);
        assert_eq!(rights.area(Area::Mail).level, Level::Full, "the rest unchanged");

        let mut hook = State::default();
        let _ = send(&mut hook, &fixture, "h1", Message::Outbound(Outbound::Free));
        assert!(hook.draft.is_none(), "a hook never sends mail");
    }

    #[test]
    fn leaving_saves_at_once_and_the_draft_shows_until_the_reload() {
        let fixture = fixture();
        let mut state = State::default();
        let _ = send(&mut state, &fixture, "c1", Message::Level(Area::Apps, Level::Read));
        let client = &fixture.clients[0];
        assert_eq!(shown(&state, client).area(Area::Apps).level, Level::Read, "shown at once");
        let action = save_now::<()>(&mut state);
        assert!(
            matches!(action, Action::Send(ref requests) if matches!(requests[..], [Request::UpdateClient { ref id, .. }] if id == "c1"))
        );
        assert!(answered(&mut state, &Ok(Response::Ok)), "the draft's answer");
        assert_eq!(shown(&state, client).area(Area::Apps).level, Level::Read, "until the reload");
        loaded(&mut state);
        assert_eq!(shown(&state, client).area(Area::Apps).level, Level::Full, "then what the core holds");
        assert!(!answered(&mut state, &Ok(Response::Ok)), "another answer is not the draft's");
    }

    #[test]
    fn a_failed_save_drops_the_draft() {
        let fixture = fixture();
        let mut state = State::default();
        let _ = send(&mut state, &fixture, "c1", Message::Level(Area::Apps, Level::Read));
        let _ = saved(&mut state, &fixture);
        assert!(answered(&mut state, &Err("forbidden".into())));
        assert_eq!(shown(&state, &fixture.clients[0]).area(Area::Apps).level, Level::Full);
    }

    #[test]
    fn a_removed_client_s_draft_is_forgotten() {
        let fixture = fixture();
        let mut state = State::default();
        let _ = send(&mut state, &fixture, "c1", Message::Level(Area::Apps, Level::Read));
        forget(&mut state, "s1");
        assert_eq!(subscription(&state).units(), 1, "another client's draft stays");
        forget(&mut state, "c1");
        assert_eq!(subscription(&state).units(), 0);
        assert_eq!(save_now::<()>(&mut state), Action::None);
    }

    #[test]
    fn the_clock_runs_only_while_a_change_waits() {
        let fixture = fixture();
        let mut state = State::default();
        assert_eq!(subscription(&state).units(), 0);
        let _ = send(&mut state, &fixture, "c1", Message::Confirm(Confirm::Never));
        assert_eq!(subscription(&state).units(), 1);
        let _ = saved(&mut state, &fixture);
        assert_eq!(subscription(&state).units(), 0);
    }

    #[test]
    fn hook_rights_cannot_be_changed() {
        let fixture = fixture();
        let mut state = State::default();
        for message in [
            Message::Level(Area::Calendar, Level::Full),
            Message::Admin(Area::Accounts, true),
            Message::All(Pick::InboxSources, true),
            Message::Command("inbox.post_item", false),
        ] {
            let _ = send(&mut state, &fixture, "h1", message);
        }
        assert!(state.draft.is_none());
        assert_eq!(subscription(&state).units(), 0);
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
        // A command switched off makes a template custom.
        let mut narrowed = RightsTemplate::ReadOnly.rights();
        narrowed.denied.insert("calendar.list_events".into());
        assert_ne!(rights_summary(&narrowed, "codex"), tr("Read only"));
    }

    #[test]
    fn repositories_come_from_the_filter_lists_and_can_be_typed() {
        let fixture = fixture();
        assert_eq!(repos_in(&fixture.task_lists), ["anna/torro", "acme"], "-repo: is not offered");
        let selection = Selection::Only(vec!["ANNA/torro".into(), "zed/tool".into()]);
        let ids: Vec<String> =
            candidates(Pick::GithubRepos, &fixture.known(), &selection).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["acme", "ANNA/torro", "zed/tool"], "each once as the selection spells it, sorted");
        assert_eq!(valid_repo(" anna/notes "), Some("anna/notes".into()));
        assert_eq!(valid_repo("acme"), Some("acme".into()));
        for bad in ["", "a/b/c", "/x", "x/", "two words", "a;b"] {
            assert_eq!(valid_repo(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn sources_include_every_client_and_the_selection_but_never_the_suite() {
        let mut fixture = fixture();
        fixture.sources.push(SourceInfo { source: SourceRef::suite(), open: 2, muted: false, health: None });
        let selection = Selection::Only(vec!["script:gone".into()]);
        let keys: Vec<String> =
            candidates(Pick::InboxSources, &fixture.known(), &selection).into_iter().map(|c| c.id).collect();
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
        let offered = candidates(Pick::InboxSources, &fixture.known(), &stale);
        let suite = offered.iter().find(|candidate| candidate.id == "suite").expect("the stale entry");
        assert_eq!(suite.detail.as_deref(), Some(tr("No longer available")));
    }

    #[test]
    fn remote_accounts_offer_their_sources_by_kind_with_the_account_below() {
        let mut fixture = fixture();
        fixture.accounts.push(account("github:anna-dev", "github", "anna-dev"));
        fixture.accounts.push(account("ntfy:push.example.org", "ntfy", "anna@push.example.org"));
        fixture.accounts.push(account("caldav:anna@posteo.de@posteo.de", "caldav", "anna@posteo.de"));
        let offered = candidates(Pick::InboxSources, &fixture.known(), &Selection::Only(Vec::new()));
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
        fixture.sources.push(SourceInfo { source: SourceRef::github("anna-dev"), open: 3, muted: false, health: None });
        let again = candidates(Pick::InboxSources, &fixture.known(), &Selection::All);
        assert_eq!(again.iter().filter(|c| c.id == "github:anna-dev").count(), 1);
    }

    #[test]
    fn segmented_controls_get_a_line_of_their_own_where_the_row_would_cut_them_off() {
        // The page widths of a narrow window (480, 16 margin) and of a medium one (1000: rail,
        // inset and 24 margin). Since issue #291 the standard's text (14) and segments are larger: in a
        // medium window of 800 the German names leave the controls no room beside them any more.
        let narrow = 480.0 - 2.0 * space::l();
        let medium = 1000.0 - torro_gui::shell::rail() - torro_gui::layout::inset() - 2.0 * space::xl();
        assert_eq!(control_column(narrow), None);
        assert_eq!(control_column(medium), Some(segment_slot()));
        // At the reading width (720) it depends on the language since issue #291: English keeps the
        // column, the longer German lines in the standard's meta (13) put the controls under their names.
        let english = torro_core::i18n::tr("Send on my behalf") == "Send on my behalf";
        let reading = control_column(torro_gui::theme::size::content_max());
        assert!(reading.is_none() || reading == Some(segment_slot()));
        if english {
            assert_eq!(reading, Some(segment_slot()));
        }
    }

    #[test]
    fn selected_ids_the_core_no_longer_knows_stay_to_be_unticked() {
        let fixture = fixture();
        let selection = Selection::Only(vec!["acc/work".into(), "acc/old".into()]);
        let offered = candidates(Pick::Calendars, &fixture.known(), &selection);
        assert_eq!(offered.len(), 3);
        assert_eq!(offered[2].detail.as_deref(), Some(tr("No longer available")));
        assert!(offered[0].color.is_some(), "calendars keep their colour");
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
        assert!(slot < torro_gui::theme::size::content_max() / 2.0, "leaves room for the area's name");
    }

    #[test]
    fn the_admin_switch_names_its_area() {
        let accounts = tr("Accounts");
        assert_eq!(admin_detail(Area::Accounts), trf("{area} · each time with your approval", &[("area", &accounts)]));
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
    }

    /// The editor builds for every kind, every naming, with every area and selection open.
    #[test]
    fn the_editor_builds_in_every_state() {
        let fixture = fixture();
        let mut state = State::default();
        for area in Area::ALL {
            let _ = send(&mut state, &fixture, "c1", Message::Expand(area));
        }
        let mut rights = RightsTemplate::FullAccess.rights();
        for pick in Pick::ALL {
            *pick.of_mut(&mut rights) = Selection::Only(Vec::new());
        }
        rights.denied.insert("calendar.delete_event".into());
        for kind in ["codex", "script", ASSISTANT_CLIENT, BOT_CLIENT] {
            for naming in [Naming::Endpoints, Naming::Tools] {
                for column in [None, Some(segment_slot())] {
                    drop(groups(&state, &rights, kind, &fixture.known(), naming, "Access", column));
                }
                drop(view(&state, &rights, kind, fixture.known(), naming, "Access"));
            }
        }
    }
}
