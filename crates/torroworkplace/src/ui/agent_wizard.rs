//! The dialogs that give a program a key (task A.7 in `docs/tasks/api-mcp.md`, task S.35 in
//! `docs/tasks/steuerzentrale.md`, concept in `docs/settings.md` › "API" and "MCP"): **"Connect
//! client"** in the header of MCP ([`State::new`]) and **"Create key"** in the header of API
//! ([`State::key`]), which is the dialog's path "Script" alone.
//!
//! Steps, in the frame of the add-account dialog (`wizard.rs`): title, a subtitle per step, errors
//! bottom left, "Back", "Cancel" and the step's primary button bottom right.
//!
//! 1. **Client** (only "Connect client"): the clients found on this computer first (Claude Code,
//!    Codex, Gemini CLI, Claude Desktop, Cursor, each with where its entry goes), then "Other MCP
//!    client", then the known clients that were not found, faded with "Not found". Detection runs
//!    in the shell ([`client_setup::Job::Detect`]); until it answers the step says it is looking.
//! 2. **Rights:** "Read only", "Read and write" (chosen at first) or "Full access", and the note
//!    that areas, calendars, lists, inbox sources, mail accounts and approvals can be narrowed on
//!    the client's page afterwards, and that sending mail asks first whichever template is chosen
//!    ([`Outbound::WithApproval`] in all of them). A script and an "Other MCP client" also need a
//!    name: it is their inbox source (`script:<name>`, `agent:mcp:<name>`), so it must differ from
//!    every other client of its kind and, for a script, from `cli` – checked here first (the
//!    message shows after the first "Create key", `torro-design` 5) and by the core. For Claude
//!    Code, Codex and Gemini CLI a check box "Report to the Notification Hub when waiting or
//!    done", ticked at first (task E2.3). "Create key" starts here and has no "Back".
//! 3. **Connecting:** `clients.create_client`, then for an agent the shell writes its MCP entry with
//!    the key ([`client_setup::Job::Connect`]). With the check box ticked a second client follows,
//!    of the kind `hook` for this agent ("Claude Code hooks", `clients.create_client` with the
//!    template "Full access", which the core narrows to `Rights::hook`: report to the inbox, see
//!    nothing), and the shell writes the agent's hooks with its key ([`client_setup::Job::Hook`]).
//!    Neither Cancel nor Escape nor a click next to the dialog interrupts this: the keys exist
//!    only in this window's memory.
//! 4. **Done:** "Restart Claude Code to use Torro Workplace." as the step's subtitle, below it a
//!    group with a row per thing that was set up, each with a check in the ok colour: where the
//!    entry and the hooks went (Codex runs new hooks only once they are trusted in its `/hooks`),
//!    or why the hooks could not be set up (a cross in the error colour): then the hook client is
//!    removed again, and the agent works without them. The keys are dropped from memory.
//!
//!    **Key:** a script's key, an "Other MCP client"'s key with the command its configuration
//!    runs ([`client_setup::mcp_command`]), or an agent's key whose entry could not be written
//!    (with the reason): shown this once with "Copy", the way a client's page shows a new key. A
//!    click next to the dialog keeps it; "Done" and Escape end it for good.
//!
//! The page that hosts the dialog keeps it in an `Option` and hands every message to [`update_in`],
//! which closes it on Cancel, Done, Escape and a click next to it as far as [`closable`] and
//! [`dismissable`] allow. [`view`] is the dialog's content; the page's `overlay` puts it on the
//! dialog surface. Its rows ([`wrap_row`]) let label and line below wrap instead of cutting them
//! off, since the dialog is only as wide as a narrow window allows; the lines below carry no full
//! stop, like the rows of the pages.

use iced::widget::space as blank;
use iced::widget::text::IntoFragment;
use iced::widget::{Space, checkbox, column, container, row, svg, text, text_input};
use iced::{Alignment, Color, Element, Fill, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, Palette, palette, size, space};
use torro_gui::{dialog, layout, style, typo, widgets};
use torro_inbox::AgentKind;
#[cfg(doc)]
use torro_ipc::api::Outbound;
use torro_ipc::api::{HOOK_CLIENT, MCP_CLIENT, RightsTemplate, SCRIPT_CLIENT};
use torro_ipc::{IssuedKey, Request, Response};

use super::{Action, Data, chevron, one_line, pressable, row_frame};
use crate::client_setup::{self, Found, Job, Method, Outcome, TOKEN_VARIABLE, Target};

/// The step the dialog is on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Client,
    Rights,
    /// `clients.create_client` is on its way.
    Creating,
    /// The shell writes the agent's MCP entry.
    Writing,
    /// `clients.create_client` for the agent's hook client is on its way.
    CreatingHooks,
    /// The shell writes the agent's hooks.
    WritingHooks,
    Done,
    /// The key is shown once: a script's, an "Other MCP client"'s, or an agent's whose entry
    /// could not be written.
    Key,
}

/// What the person connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// A client the wizard sets up itself; only one that was found can be chosen.
    Agent(Target),
    /// Any program with a key for the API ("Create key"): its key is shown to copy.
    Script,
    /// Any other MCP client ("Other MCP client", kind `mcp`): its key and command are shown to
    /// copy.
    Other,
}

impl Choice {
    /// The kind `clients.create_client` gets for it.
    fn kind(self) -> &'static str {
        match self {
            Choice::Agent(target) => target.kind(),
            Choice::Script => SCRIPT_CLIENT,
            Choice::Other => MCP_CLIENT,
        }
    }

    /// Whether the person names the client: a script and an "Other MCP client", whose name is
    /// their inbox source.
    fn named(self) -> bool {
        matches!(self, Choice::Script | Choice::Other)
    }
}

#[derive(Debug, Default)]
pub struct State {
    step: Step,
    /// "Create key" ([`State::key`]): starts on the rights of a script and has no way back to a
    /// list of clients.
    key_only: bool,
    /// What detection found; `None` until it answered.
    found: Option<Vec<Found>>,
    choice: Option<Choice>,
    template: RightsTemplate,
    /// A script's name as typed.
    name: String,
    /// "Connect" was pressed once: the name's problems show from now on.
    tried: bool,
    /// The core's or the setup's error, for the footer (Rights) or above the key (Key).
    error: Option<String>,
    /// The key from `clients.create_client`, until the entry is written or "Done".
    key: Option<IssuedKey>,
    /// "Copy" was pressed.
    copied: bool,
    /// The check box "Report to the Notification Hub when waiting or done" was unticked.
    no_hooks: bool,
    /// The agent client just created: its hooks are noted with it, so disconnecting it takes them.
    agent_client: Option<String>,
    /// The hook client just created, removed again if its hooks cannot be written.
    hook_client: Option<String>,
    /// How setting up the hooks went: where they went, or why not. `None`: not set up.
    hooks: Option<Result<String, String>>,
    /// The command an "Other MCP client" runs ([`client_setup::mcp_command`]), shown with its key.
    command: String,
    /// "Copy" next to the command was pressed.
    copied_command: bool,
}

impl State {
    /// "Connect client" on its first step, waiting for detection (the host sends
    /// [`Job::Detect`]).
    pub fn new() -> State {
        let program = std::env::current_exe().unwrap_or_else(|_| torro_core::AppId::WORKPLACE.binary.into());
        State { command: client_setup::mcp_command(&program), ..State::default() }
    }

    /// "Create key": the rights of a new script with its name, no detection and no "Back".
    pub fn key() -> State {
        State { step: Step::Rights, choice: Some(Choice::Script), key_only: true, ..State::default() }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Cancel, Done or Escape.
    Close,
    /// A click next to the dialog.
    Dismiss,
    /// Back from the rights to the clients.
    Back,
    Choose(Choice),
    Template(RightsTemplate),
    Name(String),
    /// The check box "Report to the Notification Hub when waiting or done".
    Hooks(bool),
    /// "Connect" / "Create key", or Enter in the name field.
    Connect,
    CopyKey,
    /// "Copy" next to an "Other MCP client"'s command.
    CopyCommand,
    /// Typing into the key's field changes nothing; the field always shows the key.
    KeepKey,
    /// A job the shell ran for the dialog: detection, writing the entry.
    Setup(Outcome),
}

/// Whether Cancel, Done and Escape may close the dialog now: not while the client is being
/// created or set up, since its key would be lost.
pub fn closable(state: &State) -> bool {
    !matches!(state.step, Step::Creating | Step::Writing | Step::CreatingHooks | Step::WritingHooks)
}

/// Whether a click next to the dialog closes it: on the steps that hold nothing to lose.
pub fn dismissable(state: &State) -> bool {
    matches!(state.step, Step::Client | Step::Rights | Step::Done)
}

/// Hands `message` to the dialog in `slot`, the way every page that hosts it does: Cancel, Done
/// and Escape ([`Message::Close`]) close it unless it is creating the client ([`closable`]), a
/// click next to it ([`Message::Dismiss`]) only on the steps that hold nothing to lose
/// ([`dismissable`]); everything else goes to [`update`]. Nothing happens without a dialog.
pub fn update_in(slot: &mut Option<State>, message: Message, data: &Data) -> Action {
    let Some(open) = slot.as_mut() else { return Action::None };
    let close = match message {
        Message::Close => closable(open),
        Message::Dismiss => dismissable(open),
        message => return update(open, message, data),
    };
    if close {
        *slot = None;
    }
    Action::None
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Close | Message::Dismiss | Message::KeepKey => Action::None,
        Message::Back => {
            if state.step == Step::Rights && !state.key_only {
                state.step = Step::Client;
                state.error = None;
                state.tried = false;
            }
            Action::None
        }
        Message::Choose(choice) => {
            if state.step == Step::Client && found(state, choice).is_some() {
                state.choice = Some(choice);
                state.step = Step::Rights;
            }
            Action::None
        }
        Message::Template(template) => {
            state.template = template;
            Action::None
        }
        Message::Name(name) => {
            state.name = name;
            Action::None
        }
        Message::Hooks(on) => {
            state.no_hooks = !on;
            Action::None
        }
        Message::Connect => connect(state, data),
        Message::CopyKey => match state.key.as_ref() {
            Some(key) => {
                state.copied = true;
                Action::Copy(key.expose().to_owned())
            }
            None => Action::None,
        },
        Message::CopyCommand => {
            state.copied_command = true;
            Action::Copy(state.command.clone())
        }
        Message::Setup(outcome) => setup(state, outcome),
    }
}

/// The agent whose hooks are set up after its MCP entry: the chosen client has hooks and the
/// check box is ticked.
fn hook_agent(state: &State) -> Option<AgentKind> {
    match state.choice {
        Some(Choice::Agent(target)) if !state.no_hooks => target.agent(),
        _ => None,
    }
}

/// `Some(None)` for "Script" and "Other MCP client", `Some(found)` for a found agent, `None` for an
/// agent not found.
fn found(state: &State, choice: Choice) -> Option<Option<&Found>> {
    match choice {
        Choice::Script | Choice::Other => Some(None),
        Choice::Agent(target) => state.found.as_ref()?.iter().find(|found| found.target == target).map(Some),
    }
}

/// Sends `clients.create_client` once the step is complete; a script or an "Other MCP client"
/// without a valid name gets its message instead.
fn connect(state: &mut State, data: &Data) -> Action {
    let Some(choice) = state.choice.filter(|_| state.step == Step::Rights) else { return Action::None };
    state.tried = true;
    let name = match choice {
        Choice::Agent(target) => target.label().to_string(),
        Choice::Script | Choice::Other => match name_problem(&state.name, choice.kind(), data) {
            Some(problem) => {
                state.error = Some(problem.to_string());
                return Action::None;
            }
            None => state.name.trim().to_string(),
        },
    };
    state.error = None;
    state.step = Step::Creating;
    Action::Send(Request::CreateClient {
        name,
        kind: choice.kind().to_string(),
        template: state.template,
        agent: None,
        rights: None,
    })
}

/// What is wrong with `name` for a new client of `kind` (`script` or `mcp`): empty, the name of
/// another client of that kind (in any case, as the core compares) or, for a script, `cli` (the
/// app key's source).
pub fn name_problem(name: &str, kind: &str, data: &Data) -> Option<&'static str> {
    let name = name.trim();
    let taken = (kind == SCRIPT_CLIENT && name.eq_ignore_ascii_case("cli"))
        || data.clients.iter().any(|client| client.kind == kind && client.name.eq_ignore_ascii_case(name));
    match (name.is_empty(), taken, kind == SCRIPT_CLIENT) {
        (true, _, _) => Some(tr("Enter a name.")),
        (false, true, true) => Some(tr("Another script has this name. Choose another one.")),
        (false, true, false) => Some(tr("Another MCP client has this name. Choose another one.")),
        (false, false, _) => None,
    }
}

/// The answer to a request the shell sent, while the dialog is open. `Some` when it was the
/// dialog's (then the shell shows no notice of its own) with what to do next: for an agent's new
/// key, writing its entry ([`Job::Connect`]). `None` for every other answer, and for a new key of a
/// client of another kind than the one the dialog is creating – a page's own request (a hook
/// switch, task S.35) may be answered meanwhile.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> Option<Action> {
    let expected = match state.step {
        Step::Creating => state.choice.map(Choice::kind)?,
        Step::CreatingHooks => HOOK_CLIENT,
        _ => return None,
    };
    if matches!(result, Ok(Response::ClientKey { client, .. }) if client.kind != expected) {
        return None;
    }
    match state.step {
        Step::Creating => Some(created(state, result)),
        _ => Some(hook_created(state, result)),
    }
}

/// The answer to the agent's or script's `clients.create_client`.
fn created(state: &mut State, result: &Result<Response, String>) -> Action {
    match result {
        Ok(Response::ClientKey { client, key }) => {
            let agent = match state.choice {
                Some(Choice::Agent(target)) => state.found.iter().flatten().find(|found| found.target == target),
                _ => None,
            };
            let Some(found) = agent.cloned() else {
                state.key = Some(key.clone());
                state.step = Step::Key;
                return Action::None;
            };
            state.key = Some(key.clone());
            state.agent_client = Some(client.id.clone());
            state.step = Step::Writing;
            Action::Setup(Job::Connect {
                target: found.target,
                method: found.method,
                client: client.id.clone(),
                key: key.clone(),
            })
        }
        Ok(_) => {
            state.step = Step::Rights;
            state.error = Some(tr("Torro Workplace did not answer as expected. Try again.").to_string());
            Action::None
        }
        Err(message) => {
            state.step = Step::Rights;
            state.error = Some(message.clone());
            Action::None
        }
    }
}

/// The answer to the hook client's `clients.create_client`: its key goes straight into the job
/// that writes the hooks and is not kept. A refusal ends in Done with the reason; the agent is
/// connected all the same.
fn hook_created(state: &mut State, result: &Result<Response, String>) -> Action {
    match (result, hook_agent(state)) {
        (Ok(Response::ClientKey { client, key }), Some(agent)) => {
            state.hook_client = Some(client.id.clone());
            state.step = Step::WritingHooks;
            Action::Setup(Job::Hook {
                agent,
                owner: state.agent_client.clone(),
                client: client.id.clone(),
                key: key.clone(),
            })
        }
        (Err(message), _) => {
            state.hooks = Some(Err(message.clone()));
            state.step = Step::Done;
            Action::None
        }
        _ => {
            state.hooks = Some(Err(tr("Torro Workplace did not answer as expected. Try again.").to_string()));
            state.step = Step::Done;
            Action::None
        }
    }
}

/// A job's outcome: detection fills the list; a written entry forgets the key and goes on to the
/// hook client (`clients.create_client`, kind `hook`) when the check box is ticked, else ends in
/// Done; a failed one shows the key with the reason (no hooks then: the person sets the agent up
/// by hand). Written hooks end in Done; failed ones too, with the reason, and their hook client
/// is removed again (`clients.remove_client`), since nothing uses its key.
fn setup(state: &mut State, outcome: Outcome) -> Action {
    match outcome {
        Outcome::Detected(found) => state.found = Some(found),
        Outcome::Connected(result) if state.step == Step::Writing => match result {
            Ok(()) => {
                state.key = None;
                let Some(agent) = hook_agent(state) else {
                    state.step = Step::Done;
                    return Action::None;
                };
                state.step = Step::CreatingHooks;
                return Action::Send(Request::CreateClient {
                    name: trf("{agent} hooks", &[("agent", &agent.label())]),
                    kind: HOOK_CLIENT.to_string(),
                    // The core narrows any template to `Rights::hook`; "Full access" is the one
                    // that leaves exactly that (write in the inbox, never ask first).
                    template: RightsTemplate::FullAccess,
                    agent: Some(agent),
                    rights: None,
                });
            }
            Err(message) => {
                state.error = Some(message);
                state.step = Step::Key;
            }
        },
        Outcome::Hooked(result) if state.step == Step::WritingHooks => {
            state.step = Step::Done;
            let failed = result.is_err();
            state.hooks = Some(result);
            if let Some(id) = state.hook_client.take().filter(|_| failed) {
                return Action::Send(Request::RemoveClient { id });
            }
        }
        Outcome::Connected(_) | Outcome::Hooked(_) | Outcome::Disconnected(_) | Outcome::HookNotes(_) => {}
    }
    Action::None
}

// ----- Views --------------------------------------------------------------------------------

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let title = if state.key_only { tr("Create key") } else { tr("Connect client") };
    let header =
        column![typo::text(typo::title(), title), typo::text(typo::body(), subtitle(state)).style(theme::text_2)]
            .spacing(space::xs());
    let body = match state.step {
        Step::Client => client_step(state),
        Step::Rights => rights_step(state, data),
        Step::Creating | Step::Writing | Step::CreatingHooks | Step::WritingHooks => {
            typo::text(typo::body(), trf("Connecting {name} …", &[("name", &chosen_name(state))]))
                .style(theme::text_2)
                .into()
        }
        Step::Done => done_step(state),
        Step::Key => key_step(state),
    };
    // Head and foot stay in the window; the steps scroll between them when it is low (D.12).
    dialog::frame(header, body, footer(state, data), space::xl())
}

/// "Claude Code", or the name of the script or the "Other MCP client".
fn chosen_name(state: &State) -> String {
    match state.choice {
        Some(Choice::Agent(target)) => target.label().to_string(),
        _ => state.name.trim().to_string(),
    }
}

fn subtitle(state: &State) -> String {
    let name = chosen_name(state);
    match state.step {
        Step::Client => tr("Choose the client that should use Torro Workplace through MCP.").to_string(),
        Step::Rights => match state.choice {
            Some(Choice::Script) => tr("Name the script and choose what it may do.").to_string(),
            Some(Choice::Other) => tr("Name the client and choose what it may do.").to_string(),
            _ => trf("Choose what {name} may do.", &[("name", &name)]),
        },
        Step::Creating | Step::Writing | Step::CreatingHooks | Step::WritingHooks => {
            tr("Creating a key and adding it to the configuration.").to_string()
        }
        Step::Done => trf("Restart {name} to use Torro Workplace.", &[("name", &name)]),
        Step::Key => tr("Copy the key now.").to_string(),
    }
}

/// The clients: found ones to press, "Other MCP client", and the others faded with "Not found".
fn client_step(state: &State) -> Element<'_, Message> {
    let Some(found) = &state.found else {
        return typo::text(typo::body(), tr("Looking for agents on this computer …")).style(theme::text_2).into();
    };
    let mut rows: Vec<Element<'_, Message>> = found
        .iter()
        .map(|found| {
            let row = wrap_row(None, found.target.label(), Some(found_detail(found)), chevron());
            pressable(row, Some(Message::Choose(Choice::Agent(found.target))))
        })
        .collect();
    let other = wrap_row(
        None,
        tr("Other MCP client"),
        Some(tr("Any client that speaks MCP, with a key you put in yourself").to_string()),
        chevron(),
    );
    rows.push(pressable(other, Some(Message::Choose(Choice::Other))));
    rows.extend(
        Target::ALL.into_iter().filter(|target| !found.iter().any(|found| found.target == *target)).map(missing_row),
    );
    let footer = tr("Not listed? Choose Other MCP client and put the key into its configuration yourself.");
    widgets::group(tr("MCP clients"), rows, Some(footer.to_string()))
}

/// "Found · set up with the claude command", "Found · writes ~/.cursor/mcp.json".
fn found_detail(found: &Found) -> String {
    match found.method {
        Method::Cli { .. } => trf("Found · set up with the {command} command", &[("command", &found.place)]),
        Method::Json { .. } | Method::Toml { .. } => trf("Found · writes {file}", &[("file", &found.place)]),
    }
}

/// A known client that is not on this computer: name and "Not found", faded to the disabled 40 %,
/// as high as the rows of the clients that were found.
fn missing_row(target: Target) -> Element<'static, Message> {
    let faded = |colour: fn(&Palette) -> Color| {
        move |t: &Theme| text::Style { color: Some(colour(&palette(t)).scale_alpha(style::DISABLED_ALPHA)) }
    };
    let name = one_line(typo::text(typo::body(), target.label()).style(faded(|p| p.text)));
    row_frame(
        row![name, widgets::pill_disabled(None, tr("Not found"))].spacing(space::l()).align_y(Alignment::Center),
        true,
    )
}

/// The name of a script or an "Other MCP client", and the three templates with a check at the
/// chosen one.
fn rights_step<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let mut page = column![].spacing(layout::section_gap());
    if let Some(choice) = state.choice.filter(|choice| choice.named()) {
        // An example, not a word to translate.
        let example = if choice == Choice::Script { "nightly-backup" } else { "my-editor" };
        let field = text_input(example, &state.name)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .font(typo::body().font)
            .padding(style::INPUT_PADDING)
            .width(Fill)
            .style(style::text_input)
            .on_input(Message::Name)
            .on_submit(Message::Connect);
        let footer = match state.tried.then(|| name_problem(&state.name, choice.kind(), data)).flatten() {
            Some(problem) => problem,
            None => tr("Its reports in the Notification Hub appear under this name."),
        };
        page = page.push(widgets::group(tr("Name"), vec![row_frame(field, false)], Some(footer.to_string())));
    }
    let rows = RightsTemplate::ALL
        .into_iter()
        .map(|template| {
            let mark: Element<'_, Message> = if template == state.template {
                icon::icon(Icon::Check, icon::SIZE)
                    .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) })
                    .into()
            } else {
                Space::new().width(icon::SIZE).into()
            };
            let row = wrap_row(None, template_label(template), Some(template_detail(template).to_string()), mark);
            pressable(row, Some(Message::Template(template)))
        })
        .collect();
    page = page.push(widgets::group(tr("Access"), rows, Some(rights_footer())));
    match state.choice {
        Some(Choice::Agent(target)) if target.agent().is_some() => page.push(inbox_group(state)).into(),
        _ => page.into(),
    }
}

/// The sentence under the templates: what can be narrowed later, and that no template lets an
/// agent send mail unasked (task M3.4: every template leaves "Send on my behalf" at "Ask me each
/// time"; "Full access" would otherwise suggest the opposite).
fn rights_footer() -> String {
    format!(
        "{} {}",
        tr(
            "Areas, calendars, lists, Notification Hub sources, mail accounts and what needs your approval can be narrowed on its page later."
        ),
        tr("Sending mail always asks you first.")
    )
}

/// The check box "Report to the Notification Hub when waiting or done"; the whole row ticks it.
fn inbox_group(state: &State) -> Element<'_, Message> {
    let on = !state.no_hooks;
    let check = checkbox(on)
        .size(style::CHECK_SIZE)
        // `text_2`, not `text`: a black box would be the heaviest mark of the light dialog.
        .style(|t: &Theme, status| style::checkbox(palette(t).text_2)(t, status))
        .on_toggle(Message::Hooks);
    let label = tr("Report to the Notification Hub when waiting or done");
    let detail = tr("A separate key that can only report");
    let row = wrap_row(Some(check.into()), label, Some(detail.to_string()), Space::new());
    widgets::group(tr("Notification Hub"), vec![pressable(row, Some(Message::Hooks(!on)))], None)
}

fn template_label(template: RightsTemplate) -> &'static str {
    match template {
        RightsTemplate::ReadOnly => tr("Read only"),
        RightsTemplate::ReadAndWrite => tr("Read and write"),
        RightsTemplate::FullAccess => tr("Full access"),
    }
}

pub(super) fn template_detail(template: RightsTemplate) -> &'static str {
    match template {
        RightsTemplate::ReadOnly => tr("Reads everything, changes nothing"),
        RightsTemplate::ReadAndWrite => tr("Changes everything; deleting needs approval"),
        RightsTemplate::FullAccess => tr("Changes and deletes everything without asking"),
    }
}

/// What was set up, a row each with a check in the ok colour: the entry, the hooks (for Codex
/// with how to trust them) or, with a cross in the error colour, why the hooks could not be set
/// up. Below the group how to undo it. ("Restart …" is the step's subtitle.)
fn done_step(state: &State) -> Element<'_, Message> {
    let place = state.choice.and_then(|choice| found(state, choice)).flatten();
    let mut rows = Vec::new();
    match place {
        Some(found) if matches!(found.method, Method::Cli { .. }) => {
            let added = trf("Added with the {command} command", &[("command", &found.place)]);
            rows.push(wrap_row(Some(status_mark(Icon::CircleCheck, |p| p.ok)), added, None, Space::new()));
        }
        Some(found) => {
            let added = trf("Added to {file}", &[("file", &found.place)]);
            rows.push(wrap_row(Some(status_mark(Icon::CircleCheck, |p| p.ok)), added, None, Space::new()));
        }
        None => {}
    }
    match &state.hooks {
        Some(Ok(file)) => {
            let added = trf("Hooks added to {file}", &[("file", file)]);
            let trust = (state.choice == Some(Choice::Agent(Target::Codex)))
                .then(|| tr("Codex runs them only after you trust them: type /hooks in Codex").to_string());
            rows.push(wrap_row(Some(status_mark(Icon::CircleCheck, |p| p.ok)), added, trust, Space::new()));
        }
        Some(Err(error)) => {
            let failed = tr("Notification Hub reports could not be set up");
            rows.push(wrap_row(
                Some(status_mark(Icon::CircleX, |p| p.error)),
                failed,
                Some(error.clone()),
                Space::new(),
            ));
        }
        None => {}
    }
    let undo = tr("To undo, choose Disconnect on its page.");
    if rows.is_empty() {
        return typo::text(typo::body(), undo).style(theme::text_2).into();
    }
    widgets::group(tr("Configuration"), rows, Some(undo.to_string()))
}

/// A status icon in front of a row: 16, in the colour `colour` picks from the palette.
fn status_mark<'a>(which: Icon, colour: fn(&Palette) -> Color) -> Element<'a, Message> {
    icon::icon(which, icon::SIZE).style(move |t: &Theme, _| svg::Style { color: Some(colour(&palette(t))) }).into()
}

/// A row of a settings group in the dialog, with the metrics of `setting_row` (16 at the sides,
/// 12 after the `mark`, 16 before the `control`, 44 high, 56 with a `detail`), but label and detail
/// wrap instead of being cut off: the dialog is only as wide as the window, and a translation or a
/// long path must stay readable. The row grows by a line then. The label takes the text colour of
/// a surrounding button ([`pressable`]); the detail is `META` in `text_2`.
fn wrap_row<'a>(
    mark: Option<Element<'a, Message>>,
    label: impl IntoFragment<'a>,
    detail: Option<String>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let padding = row_padding(detail.is_some());
    let mut lines = column![typo::text(typo::body(), label)];
    if let Some(detail) = detail {
        lines = lines.push(typo::text(typo::meta(), detail).style(theme::text_2));
    }
    let text = row![].push(mark).push(lines.width(Fill)).spacing(space::m()).align_y(Alignment::Center);
    let content = row![text.width(Fill), control.into()].spacing(space::l()).align_y(Alignment::Center);
    container(content).width(Fill).padding([padding, space::l()]).into()
}

/// The padding above and below a [`wrap_row`]: what makes it 44 high with one line of text and 56
/// with a detail, as long as nothing wraps.
fn row_padding(detail: bool) -> f32 {
    let line = |level: typo::Level| level.line_height.to_absolute(level.size).0;
    match detail {
        true => (size::header() + space::m() - line(typo::body()) - line(typo::meta())) / 2.0,
        false => (size::header() - line(typo::body())) / 2.0,
    }
}

/// The key this once: why it is shown, the key in a field to select, and "Copy"; for an "Other MCP
/// client" also the command its configuration runs, with "Copy" of its own.
fn key_step(state: &State) -> Element<'_, Message> {
    let mut body = column![].spacing(space::l());
    if let Some(error) = &state.error {
        let problem = trf("The configuration could not be written: {error}", &[("error", error)]);
        body = body.push(
            typo::text(typo::body(), problem).style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }),
        );
        body = body.push(
            typo::text(typo::body(), tr("Put this key into its configuration yourself. It is shown only this once."))
                .style(theme::text_2),
        );
    } else if state.choice == Some(Choice::Other) {
        body = body.push(
            typo::text(
                typo::body(),
                trf(
                    "Put this command into the client's configuration and pass the key as {variable}. The key is shown only this once; Torro Workplace keeps only a fingerprint of it.",
                    &[("variable", &TOKEN_VARIABLE)],
                ),
            )
            .style(theme::text_2),
        );
        body = body.push(command_line(&state.command, state.copied_command, Message::CopyCommand, Message::KeepKey));
    } else {
        body = body.push(
            typo::text(
                typo::body(),
                trf(
                    "Pass it to your program as {variable}. It is shown only this once; Torro Workplace keeps only a fingerprint of it.",
                    &[("variable", &TOKEN_VARIABLE)],
                ),
            )
            .style(theme::text_2),
        );
    }
    let key = state.key.as_ref().map_or("", IssuedKey::expose);
    body.push(copy_line(tr("Key"), key, state.copied, Message::CopyKey, Message::KeepKey)).into()
}

/// `text` (a key) under its `label` in `CAPTION` (`text_3`), in a field to select, in tabular
/// figures – it is read and compared character by character –, and "Copy" sending `copy`, which says
/// "Copied" once it was pressed. Typing into the field sends `keep`, which must change nothing.
pub(super) fn copy_line<'a, M: Clone + 'a>(
    label: &'a str,
    text: &'a str,
    copied: bool,
    copy: M,
    keep: M,
) -> Element<'a, M> {
    copy_field(label, text.to_string(), None, copied, copy, keep)
}

/// The command an MCP client runs ([`crate::client_setup::mcp_command`]) as [`copy_line`] under
/// "Command", its path cut at the front ([`short_command`]) so the end that tells what runs – the
/// program and `mcp` – always shows; the whole command is the field's tooltip, and "Copy" copies all
/// of it (the caller's `copy`).
pub(super) fn command_line<'a, M: Clone + 'a>(command: &'a str, copied: bool, copy: M, keep: M) -> Element<'a, M> {
    let shown = short_command(command);
    let whole = (shown != command).then_some(command);
    copy_field(tr("Command"), shown, whole, copied, copy, keep)
}

/// `command` with the folders of its program left out: "…/torroworkplace mcp" for
/// "/Users/anna/bin/torroworkplace mcp" (quotes of a path with spaces dropped, it is only shown);
/// as it is when it names no folder.
fn short_command(command: &str) -> String {
    match command.rfind(['/', '\\']) {
        Some(at) => {
            let separator = &command[at..at + 1];
            let rest: String = command[at + 1..].chars().filter(|c| !matches!(c, '\'' | '"')).collect();
            format!("…{separator}{rest}")
        }
        None => command.to_string(),
    }
}

/// The parts of [`copy_line`] and [`command_line`]: `shown` in the field, `whole` as its tooltip
/// where the field shows less. "Copy" is a bar button, as high as the field (32).
fn copy_field<'a, M: Clone + 'a>(
    label: &'a str,
    shown: String,
    whole: Option<&'a str>,
    copied: bool,
    copy: M,
    keep: M,
) -> Element<'a, M> {
    let field = text_input("", &shown)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().tabular().font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        // Without an edit handler the field cannot take focus, so the text could not be selected.
        // Edits change nothing: the field always shows the text.
        .on_input(move |_| keep.clone());
    let field: Element<'a, M> = match whole {
        Some(whole) => widgets::with_tooltip(field, whole),
        None => field.into(),
    };
    let button = match copied {
        true => widgets::bar_button(Icon::Check, tr("Copied"), true, style::button::secondary, Some(copy)),
        false => widgets::bar_button(Icon::Copy, tr("Copy"), true, style::button::secondary, Some(copy)),
    };
    let line = row![field, button].spacing(space::s()).align_y(Alignment::Center);
    column![typo::text(typo::caption(), label).style(theme::text_3), line].spacing(space::xs()).into()
}

/// The error left (on the rights step), "Back", "Cancel" and the step's primary button right.
fn footer<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let error: Element<'a, Message> = match (&state.error, state.step) {
        (Some(error), Step::Rights) => typo::text(typo::body(), error.as_str())
            .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) })
            .width(Fill)
            .into(),
        _ => blank::horizontal().into(),
    };
    let back = (state.step == Step::Rights && !state.key_only)
        .then(|| widgets::text_button(None, tr("Back"), style::button::secondary, Some(Message::Back)));
    let cancel = matches!(state.step, Step::Client | Step::Rights)
        .then(|| widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Close)));
    let primary = |label, message| Some(widgets::text_button(None, label, style::button::primary, message));
    let next = match (state.step, state.choice) {
        (Step::Rights, Some(choice)) if choice.named() => {
            let ready = !state.tried || name_problem(&state.name, choice.kind(), data).is_none();
            primary(tr("Create key"), ready.then_some(Message::Connect))
        }
        (Step::Rights, _) => primary(tr("Connect"), Some(Message::Connect)),
        (Step::Done | Step::Key, _) => primary(tr("Done"), Some(Message::Close)),
        (Step::Client | Step::Creating | Step::Writing | Step::CreatingHooks | Step::WritingHooks, _) => None,
    };
    row![error].push(back).push(cancel).push(next).spacing(space::s()).align_y(Alignment::Center).into()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use torro_ipc::{ClientInfo, Key};

    use super::*;
    use crate::client_setup::{Tool, demo};
    use torro_ipc::api::HOOK_CLIENT;

    fn detected() -> State {
        let mut state = State::new();
        assert!(matches!(update(&mut state, Message::Setup(demo(&Job::Detect)), &Data::default()), Action::None));
        state
    }

    fn client(id: &str, name: &str, kind: &str) -> ClientInfo {
        ClientInfo {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            created: Utc::now(),
            last_used: None,
            rights: RightsTemplate::ReadAndWrite.rights(),
            agent: None,
        }
    }

    /// #231: the command shows its end – program and `mcp` –, the folders before it cut off.
    #[test]
    fn the_command_is_cut_at_the_front() {
        assert_eq!(short_command("/Users/anna/bin/torroworkplace mcp"), "…/torroworkplace mcp");
        assert_eq!(short_command("'/Users/anna/my apps/torroworkplace' mcp"), "…/torroworkplace mcp");
        assert_eq!(short_command(r#""C:\Program Files\torroworkplace.exe" mcp"#), "…\\torroworkplace.exe mcp");
        assert_eq!(short_command("torroworkplace mcp"), "torroworkplace mcp");
        let program = std::path::Path::new("/opt/torro/torroworkplace");
        assert!(short_command(&crate::client_setup::mcp_command(program)).ends_with("/torroworkplace mcp"));
    }

    #[test]
    fn every_template_sends_mail_only_after_asking_and_the_step_says_so() {
        for template in RightsTemplate::ALL {
            assert_eq!(template.rights().outbound, torro_ipc::api::Outbound::WithApproval, "{template:?}");
        }
        assert!(rights_footer().contains(tr("Sending mail always asks you first.")));
        let state = detected();
        drop(rights_step(&state, &Data::default()));
    }

    fn key_answer(id: &str, name: &str, kind: &str) -> Result<Response, String> {
        Ok(Response::ClientKey { client: client(id, name, kind), key: IssuedKey::new(Key::new("torro_n1_secret")) })
    }

    #[test]
    fn an_agent_is_created_with_its_template_then_its_entry_is_written() {
        let data = Data::default();
        let mut state = detected();
        let _ = update(&mut state, Message::Choose(Choice::Agent(Target::ClaudeCode)), &data);
        assert_eq!(state.step, Step::Rights);
        assert_eq!(state.template, RightsTemplate::ReadAndWrite, "the default template");
        let _ = update(&mut state, Message::Template(RightsTemplate::ReadOnly), &data);
        assert!(!state.no_hooks, "reporting to the inbox is ticked at first");
        let _ = update(&mut state, Message::Hooks(false), &data);
        let action = update(&mut state, Message::Connect, &data);
        assert!(matches!(
            action,
            Action::Send(Request::CreateClient {
                ref name,
                ref kind,
                template: RightsTemplate::ReadOnly,
                agent: None,
                rights: None,
            }) if name == "Claude Code" && kind == "claude-code"
        ));
        assert!(!closable(&state) && !dismissable(&state), "nothing interrupts the creation");

        let next = answered(&mut state, &key_answer("n1", "Claude Code", "claude-code"));
        match next {
            Some(Action::Setup(Job::Connect { target, method, client, key })) => {
                assert_eq!((target, client.as_str()), (Target::ClaudeCode, "n1"));
                assert!(matches!(method, Method::Cli { tool: Tool::Claude, .. }));
                assert_eq!(key.expose(), "torro_n1_secret");
            }
            other => panic!("expected the entry to be written, got {other:?}"),
        }
        assert_eq!(state.step, Step::Writing);
        assert!(answered(&mut state, &Ok(Response::Ok)).is_none(), "other answers are not the dialog's");

        let _ = update(&mut state, Message::Setup(Outcome::Connected(Ok(()))), &data);
        assert_eq!(state.step, Step::Done);
        assert!(state.key.is_none(), "the key is forgotten once it is in the configuration");
        assert!(closable(&state) && dismissable(&state));
        drop(view(&state, &data));
    }

    /// Claude Code chosen with the defaults, its client created and its MCP entry written.
    fn connected_claude() -> State {
        let data = Data::default();
        let mut state = detected();
        let _ = update(&mut state, Message::Choose(Choice::Agent(Target::ClaudeCode)), &data);
        drop(view(&state, &data));
        let _ = update(&mut state, Message::Connect, &data);
        let _ = answered(&mut state, &key_answer("n1", "Claude Code", "claude-code"));
        state
    }

    #[test]
    fn with_the_check_box_a_hook_client_follows_and_its_hooks_are_written() {
        let data = Data::default();
        let mut state = connected_claude();
        let action = update(&mut state, Message::Setup(Outcome::Connected(Ok(()))), &data);
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
        assert_eq!(state.step, Step::CreatingHooks);
        assert!(state.key.is_none(), "the agent's key is forgotten once it is in the configuration");
        assert!(!closable(&state) && !dismissable(&state), "nothing interrupts the hook client");

        let mut hook = key_answer("h1", &expected_name, HOOK_CLIENT);
        if let Ok(Response::ClientKey { client, .. }) = hook.as_mut() {
            client.agent = Some(AgentKind::ClaudeCode);
        }
        match answered(&mut state, &hook) {
            Some(Action::Setup(Job::Hook { agent, owner, client, key })) => {
                assert_eq!((agent, owner.as_deref(), client.as_str()), (AgentKind::ClaudeCode, Some("n1"), "h1"));
                assert_eq!(key.expose(), "torro_n1_secret");
            }
            other => panic!("expected the hooks to be written, got {other:?}"),
        }
        assert_eq!(state.step, Step::WritingHooks);
        assert!(!closable(&state));

        let ok = Outcome::Hooked(Ok("~/.claude/settings.json".into()));
        assert!(matches!(update(&mut state, Message::Setup(ok), &data), Action::None));
        assert_eq!(state.step, Step::Done);
        assert_eq!(state.hooks, Some(Ok("~/.claude/settings.json".into())));
        assert!(closable(&state) && dismissable(&state));
        drop(view(&state, &data));
    }

    #[test]
    fn failed_hooks_end_in_done_with_the_reason_and_their_client_goes() {
        let data = Data::default();
        let mut state = connected_claude();
        let _ = update(&mut state, Message::Setup(Outcome::Connected(Ok(()))), &data);
        let _ = answered(&mut state, &key_answer("h1", "Claude Code hooks", HOOK_CLIENT));
        let failed = Outcome::Hooked(Err("settings.json could not be read".into()));
        let action = update(&mut state, Message::Setup(failed), &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "h1"));
        assert_eq!(state.step, Step::Done);
        assert_eq!(state.hooks, Some(Err("settings.json could not be read".into())));
        assert!(answered(&mut state, &Ok(Response::Ok)).is_none(), "the removal's answer is not the dialog's");
        drop(view(&state, &data));

        // The core refuses the hook client: Done with its message, nothing to remove.
        let mut refused = connected_claude();
        let _ = update(&mut refused, Message::Setup(Outcome::Connected(Ok(()))), &data);
        assert!(matches!(answered(&mut refused, &Err("busy".into())), Some(Action::None)));
        assert_eq!((refused.step, refused.hooks.clone()), (Step::Done, Some(Err("busy".into()))));
    }

    #[test]
    fn apps_without_hooks_have_no_check_box() {
        let data = Data::default();
        let mut state = detected();
        let _ = update(&mut state, Message::Choose(Choice::Agent(Target::Cursor)), &data);
        assert_eq!(hook_agent(&state), None);
        let _ = update(&mut state, Message::Connect, &data);
        let _ = answered(&mut state, &key_answer("n1", "Cursor", "cursor"));
        let _ = update(&mut state, Message::Setup(Outcome::Connected(Ok(()))), &data);
        assert_eq!((state.step, state.hooks.clone()), (Step::Done, None));
    }

    #[test]
    fn a_failed_entry_shows_the_key_with_the_reason() {
        let data = Data::default();
        let mut state = detected();
        let _ = update(&mut state, Message::Choose(Choice::Agent(Target::Cursor)), &data);
        let _ = update(&mut state, Message::Connect, &data);
        let _ = answered(&mut state, &key_answer("n1", "Cursor", "cursor"));
        let _ = update(&mut state, Message::Setup(Outcome::Connected(Err("mcp.json could not be read".into()))), &data);
        assert_eq!(state.step, Step::Key);
        assert_eq!(state.error.as_deref(), Some("mcp.json could not be read"));
        assert!(closable(&state) && !dismissable(&state), "a stray click keeps the key");
        let copy = update(&mut state, Message::CopyKey, &data);
        assert!(matches!(copy, Action::Copy(ref text) if text == "torro_n1_secret"));
        drop(view(&state, &data));
    }

    /// Task S.35: "Create key" is the path "Script" alone – it starts on the rights, with no way
    /// back to a list of clients.
    #[test]
    fn a_script_needs_a_free_name_and_shows_its_key() {
        let data = Data { clients: vec![client("s1", "backup", "script")], ..Data::default() };
        let mut state = State::key();
        assert_eq!((state.step, state.choice), (Step::Rights, Some(Choice::Script)));
        let _ = update(&mut state, Message::Back, &data);
        assert_eq!(state.step, Step::Rights, "no Back in Create key");
        assert!(name_problem("", "script", &data).is_some());
        assert!(name_problem("CLI", "script", &data).is_some(), "the app key's source");
        assert!(name_problem(" Backup ", "script", &data).is_some(), "another script's name in any case");
        assert_eq!(name_problem("nightly", "script", &data), None);
        assert_eq!(name_problem("backup", "mcp", &data), None, "a script's name is free for an MCP client");
        assert_eq!(name_problem("cli", "mcp", &data), None, "cli is only the app key's script source");
        drop(view(&state, &data));

        assert!(matches!(update(&mut state, Message::Connect, &data), Action::None), "no name yet");
        assert_eq!(state.error.as_deref(), Some(tr("Enter a name.")));
        let _ = update(&mut state, Message::Name(" nightly ".into()), &data);
        let action = update(&mut state, Message::Connect, &data);
        assert!(matches!(
            action,
            Action::Send(Request::CreateClient { ref name, ref kind, .. }) if name == "nightly" && kind == "script"
        ));
        assert!(matches!(answered(&mut state, &key_answer("s2", "nightly", "script")), Some(Action::None)));
        assert_eq!(state.step, Step::Key);
        assert!(state.error.is_none());
        drop(view(&state, &data));
    }

    /// Task S.35: "Connect client" offers "Other MCP client" instead of "Script": a name checked
    /// against the other clients of the kind `mcp`, then its key with the command to copy.
    #[test]
    fn another_mcp_client_gets_a_free_name_its_key_and_the_command() {
        let data = Data { clients: vec![client("m1", "Zed", MCP_CLIENT)], ..Data::default() };
        let mut state = detected();
        let _ = update(&mut state, Message::Choose(Choice::Other), &data);
        assert_eq!(state.step, Step::Rights);
        let _ = update(&mut state, Message::Name("zed".into()), &data);
        assert!(matches!(update(&mut state, Message::Connect, &data), Action::None));
        assert_eq!(state.error.as_deref(), Some(tr("Another MCP client has this name. Choose another one.")));
        let _ = update(&mut state, Message::Name(" Helix ".into()), &data);
        let action = update(&mut state, Message::Connect, &data);
        assert!(matches!(
            action,
            Action::Send(Request::CreateClient { ref name, ref kind, agent: None, .. }) if name == "Helix" && kind == MCP_CLIENT
        ));
        // A new key of another kind is not the dialog's: a page's hook switch may be answered first.
        assert!(answered(&mut state, &key_answer("h9", "Claude Code hooks", HOOK_CLIENT)).is_none());
        assert!(matches!(answered(&mut state, &key_answer("m2", "Helix", MCP_CLIENT)), Some(Action::None)));
        assert_eq!(state.step, Step::Key);
        assert!(state.command.ends_with(" mcp"), "{}", state.command);
        let copy = update(&mut state, Message::CopyCommand, &data);
        assert!(matches!(copy, Action::Copy(ref text) if *text == state.command));
        assert!(
            matches!(update(&mut state, Message::CopyKey, &data), Action::Copy(ref key) if key == "torro_n1_secret")
        );
        drop(view(&state, &data));
    }

    #[test]
    fn the_host_closes_the_dialog_only_when_nothing_is_lost() {
        let data = Data::default();
        let mut slot = Some(detected());
        let _ = update_in(&mut slot, Message::Dismiss, &data);
        assert!(slot.is_none(), "a click next to the first step closes it");
        let mut slot = Some(State::key());
        let _ = update_in(&mut slot, Message::Name("nightly".into()), &data);
        let _ = update_in(&mut slot, Message::Connect, &data);
        let _ = update_in(&mut slot, Message::Close, &data);
        assert!(slot.is_some(), "not while the key is being created");
        let _ = update_in(&mut slot, Message::Setup(Outcome::Connected(Ok(()))), &data);
        assert!(matches!(update_in(&mut None, Message::Close, &data), Action::None));
    }

    #[test]
    fn the_cores_refusal_goes_back_to_the_rights_with_its_message() {
        let data = Data::default();
        let mut state = detected();
        let _ = update(&mut state, Message::Choose(Choice::Agent(Target::Codex)), &data);
        let _ = update(&mut state, Message::Connect, &data);
        assert!(matches!(answered(&mut state, &Err("forbidden".into())), Some(Action::None)));
        assert_eq!((state.step, state.error.as_deref()), (Step::Rights, Some("forbidden")));
        let _ = update(&mut state, Message::Back, &data);
        assert_eq!((state.step, state.error.as_deref()), (Step::Client, None));
    }

    #[test]
    fn only_found_agents_can_be_chosen() {
        let data = Data::default();
        let mut waiting = State::new();
        let _ = update(&mut waiting, Message::Choose(Choice::Agent(Target::ClaudeCode)), &data);
        assert_eq!(waiting.step, Step::Client, "nothing is known before detection answered");
        drop(view(&waiting, &data));

        let mut state = detected();
        drop(view(&state, &data));
        let _ = update(&mut state, Message::Choose(Choice::Agent(Target::ClaudeDesktop)), &data);
        assert_eq!(state.step, Step::Client, "the demo did not find Claude Desktop");
        let _ = update(&mut state, Message::Choose(Choice::Other), &data);
        assert_eq!(state.step, Step::Rights);
        drop(view(&state, &data));
    }

    #[test]
    fn details_say_where_the_entry_goes() {
        let Outcome::Detected(found) = demo(&Job::Detect) else { panic!("detection") };
        assert_eq!(
            found_detail(&found[0]),
            trf("Found · set up with the {command} command", &[("command", &"claude")])
        );
        assert_eq!(found_detail(&found[2]), trf("Found · writes {file}", &[("file", &"~/.cursor/mcp.json")]));
    }
}
