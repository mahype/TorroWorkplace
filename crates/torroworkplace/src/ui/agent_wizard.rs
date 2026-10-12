//! The "Connect agent" dialog (task A.7 in `docs/tasks/api-mcp.md`, concept in `docs/api.md`,
//! "Steuerzentrale: Abschnitt „Agents & API“"), opened from the header of the agents page.
//!
//! Steps, in the frame of the add-account dialog (`wizard.rs`): title, a subtitle per step, errors
//! bottom left, "Back", "Cancel" and the step's primary button bottom right.
//!
//! 1. **Client:** the clients found on this computer first (Claude Code, Codex, Claude Desktop,
//!    Cursor, each with where its entry goes), then "Script", then the known clients that were not
//!    found, faded with "Not found". Detection runs in the shell ([`client_setup::Job::Detect`]);
//!    until it answers the step says it is looking.
//! 2. **Rights:** "Read only", "Read and write" (chosen at first) or "Full access", and the note
//!    that areas, calendars, lists, inbox sources and approvals can be narrowed on the agent's page
//!    afterwards. A script also needs a name: it is its inbox source (`script:<name>`), so it must
//!    differ from every other script and from `cli` – checked here first (the message shows after
//!    the first "Create key", `torro-design` 5) and by the core. For Claude Code, Codex and Gemini
//!    CLI a check box "Report to Inbox when waiting or done", ticked at first (task E2.3).
//! 3. **Connecting:** `clients.create_client`, then for an agent the shell writes its MCP entry with
//!    the key ([`client_setup::Job::Connect`]). With the check box ticked a second client follows,
//!    of the kind `hook` for this agent ("Claude Code hooks", `clients.create_client` with the
//!    template "Full access", which the core narrows to `Rights::hook`: report to the inbox, see
//!    nothing), and the shell writes the agent's hooks with its key ([`client_setup::Job::Hook`]).
//!    Neither Cancel nor Escape nor a click next to the dialog interrupts this: the keys exist
//!    only in this window's memory.
//! 4. **Done:** "Restart Claude Code to use Torro Workplace.", where the entry and the hooks went
//!    (Codex runs new hooks only once they are trusted in its `/hooks`), or why the hooks could
//!    not be set up: then the hook client is removed again, and the agent works without them. The
//!    keys are dropped from memory.
//!
//!    **Key:** a script's key, or an agent's key whose entry could not be written (with the
//!    reason): shown this once with "Copy", the way the agents page shows a new key. A click next
//!    to the dialog keeps it; "Done" and Escape end it for good.
//!
//! `Message::Close` and `Message::Dismiss` are handled by `agents.rs`, which asks [`closable`]
//! and [`dismissable`] first. [`view`] is the dialog's content; `agents::overlay` puts it on the
//! dialog surface.

use iced::widget::space as blank;
use iced::widget::{Space, checkbox, column, row, svg, text, text_input};
use iced::{Alignment, Color, Element, Fill, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, Palette, palette, space};
use torro_gui::{style, typo, widgets};
use torro_inbox::AgentKind;
use torro_ipc::api::{HOOK_CLIENT, RightsTemplate};
use torro_ipc::{IssuedKey, Request, Response};

use super::{Action, Data, chevron, one_line, pressable, row_frame, setting_row};
use crate::client_setup::{Found, Job, Method, Outcome, TOKEN_VARIABLE, Target};

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
    /// The key is shown once: a script's, or an agent's whose entry could not be written.
    Key,
}

/// What the person connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// A client the wizard sets up itself; only one that was found can be chosen.
    Agent(Target),
    /// Any program: its key is shown to copy.
    Script,
}

#[derive(Debug, Default)]
pub struct State {
    step: Step,
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
    /// The check box "Report to Inbox when waiting or done" was unticked.
    no_hooks: bool,
    /// The agent client just created: its hooks are noted with it, so disconnecting it takes them.
    agent_client: Option<String>,
    /// The hook client just created, removed again if its hooks cannot be written.
    hook_client: Option<String>,
    /// How setting up the hooks went: where they went, or why not. `None`: not set up.
    hooks: Option<Result<String, String>>,
}

impl State {
    /// A dialog on its first step, waiting for detection.
    pub fn new() -> State {
        State::default()
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
    /// The check box "Report to Inbox when waiting or done".
    Hooks(bool),
    /// "Connect" / "Create key", or Enter in the name field.
    Connect,
    CopyKey,
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

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Close | Message::Dismiss | Message::KeepKey => Action::None,
        Message::Back => {
            if state.step == Step::Rights {
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

/// `Some(None)` for "Script", `Some(found)` for a found agent, `None` for an agent not found.
fn found(state: &State, choice: Choice) -> Option<Option<&Found>> {
    match choice {
        Choice::Script => Some(None),
        Choice::Agent(target) => state.found.as_ref()?.iter().find(|found| found.target == target).map(Some),
    }
}

/// Sends `clients.create_client` once the step is complete; a script without a valid name gets
/// its message instead.
fn connect(state: &mut State, data: &Data) -> Action {
    let Some(choice) = state.choice.filter(|_| state.step == Step::Rights) else { return Action::None };
    state.tried = true;
    let (name, kind) = match choice {
        Choice::Agent(target) => (target.label().to_string(), target.kind()),
        Choice::Script => match name_problem(&state.name, data) {
            Some(problem) => {
                state.error = Some(problem.to_string());
                return Action::None;
            }
            None => (state.name.trim().to_string(), "script"),
        },
    };
    state.error = None;
    state.step = Step::Creating;
    Action::Send(Request::CreateClient { name, kind: kind.to_string(), template: state.template, agent: None })
}

/// What is wrong with `name` for a new script: empty, `cli` (the app key's source) or the name of
/// another script (in any case, as the core compares).
pub fn name_problem(name: &str, data: &Data) -> Option<&'static str> {
    let name = name.trim();
    let taken = name.eq_ignore_ascii_case("cli")
        || data.clients.iter().any(|client| client.kind == "script" && client.name.eq_ignore_ascii_case(name));
    if name.is_empty() {
        Some(tr("Enter a name."))
    } else if taken {
        Some(tr("Another script has this name. Choose another one."))
    } else {
        None
    }
}

/// The answer to a request the shell sent, while the dialog is open. `Some` when it was the
/// dialog's (then the shell shows no notice of its own) with what to do next: for an agent's new
/// key, writing its entry ([`Job::Connect`]). `None` for every other answer.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> Option<Action> {
    match state.step {
        Step::Creating => Some(created(state, result)),
        Step::CreatingHooks => Some(hook_created(state, result)),
        _ => None,
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
        Outcome::Connected(_) | Outcome::Hooked(_) | Outcome::Disconnected(_) => {}
    }
    Action::None
}

// ----- Views --------------------------------------------------------------------------------

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let header = column![
        typo::text(typo::TITLE, tr("Connect agent")),
        typo::text(typo::BODY, subtitle(state)).style(theme::text_2)
    ]
    .spacing(space::XS);
    let body = match state.step {
        Step::Client => client_step(state),
        Step::Rights => rights_step(state, data),
        Step::Creating | Step::Writing | Step::CreatingHooks | Step::WritingHooks => {
            typo::text(typo::BODY, trf("Connecting {name} …", &[("name", &chosen_name(state))]))
                .style(theme::text_2)
                .into()
        }
        Step::Done => done_step(state),
        Step::Key => key_step(state),
    };
    column![header, body, footer(state, data)].spacing(space::XL).into()
}

/// "Claude Code", or the script's name.
fn chosen_name(state: &State) -> String {
    match state.choice {
        Some(Choice::Agent(target)) => target.label().to_string(),
        _ => state.name.trim().to_string(),
    }
}

fn subtitle(state: &State) -> String {
    let name = chosen_name(state);
    match state.step {
        Step::Client => tr("Choose the agent or program that should use Torro Workplace.").to_string(),
        Step::Rights => match state.choice {
            Some(Choice::Script) => tr("Name the script and choose what it may do.").to_string(),
            _ => trf("Choose what {name} may do.", &[("name", &name)]),
        },
        Step::Creating | Step::Writing | Step::CreatingHooks | Step::WritingHooks => {
            tr("Creating a key and adding it to the configuration.").to_string()
        }
        Step::Done => trf("{name} is connected.", &[("name", &name)]),
        Step::Key => tr("Copy the key now.").to_string(),
    }
}

/// The clients: found ones to press, "Script", and the others faded with "Not found".
fn client_step(state: &State) -> Element<'_, Message> {
    let Some(found) = &state.found else {
        return typo::text(typo::BODY, tr("Looking for agents on this computer …")).style(theme::text_2).into();
    };
    let mut rows: Vec<Element<'_, Message>> = found
        .iter()
        .map(|found| {
            let row = setting_row(None, found.target.label(), Some(found_detail(found)), chevron());
            pressable(row, Some(Message::Choose(Choice::Agent(found.target))))
        })
        .collect();
    let script = setting_row(None, tr("Script"), Some(tr("A key for your own program").to_string()), chevron());
    rows.push(pressable(script, Some(Message::Choose(Choice::Script))));
    rows.extend(
        Target::ALL.into_iter().filter(|target| !found.iter().any(|found| found.target == *target)).map(missing_row),
    );
    let footer = tr("Not listed? Choose Script and put the key into its configuration yourself.");
    widgets::group(tr("Agents and programs"), rows, Some(footer.to_string()))
}

/// "Found · set up with the claude command", "Found · writes ~/.cursor/mcp.json".
fn found_detail(found: &Found) -> String {
    match found.method {
        Method::Cli { .. } => trf("Found · set up with the {command} command", &[("command", &found.place)]),
        Method::Json { .. } | Method::Toml { .. } => trf("Found · writes {file}", &[("file", &found.place)]),
    }
}

/// A known client that is not on this computer: name and "Not found", faded to the disabled 40 %.
fn missing_row(target: Target) -> Element<'static, Message> {
    let faded = |colour: fn(&Palette) -> Color| {
        move |t: &Theme| text::Style { color: Some(colour(palette(t)).scale_alpha(style::DISABLED_ALPHA)) }
    };
    let name = one_line(typo::text(typo::BODY, target.label()).style(faded(|p| p.text)));
    row_frame(
        row![name, widgets::pill_disabled(None, tr("Not found"))].spacing(space::L).align_y(Alignment::Center),
        false,
    )
}

/// A script's name, and the three templates with a check at the chosen one.
fn rights_step<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let mut page = column![].spacing(space::XL);
    if state.choice == Some(Choice::Script) {
        let field = text_input("nightly-backup", &state.name)
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .font(typo::BODY.font)
            .padding(style::INPUT_PADDING)
            .width(Fill)
            .style(style::text_input)
            .on_input(Message::Name)
            .on_submit(Message::Connect);
        let footer = match state.tried.then(|| name_problem(&state.name, data)).flatten() {
            Some(problem) => problem,
            None => tr("Its reports in the inbox appear under this name."),
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
            let row = setting_row(None, template_label(template), Some(template_detail(template).to_string()), mark);
            pressable(row, Some(Message::Template(template)))
        })
        .collect();
    let footer =
        tr("Areas, calendars, lists, inbox sources and what needs your approval can be narrowed on its page later.");
    page = page.push(widgets::group(tr("Access"), rows, Some(footer.to_string())));
    match state.choice {
        Some(Choice::Agent(target)) if target.agent().is_some() => page.push(inbox_group(state)).into(),
        _ => page.into(),
    }
}

/// The check box "Report to Inbox when waiting or done"; the whole row ticks it.
fn inbox_group(state: &State) -> Element<'_, Message> {
    let on = !state.no_hooks;
    let check = checkbox(on)
        .size(style::CHECK_SIZE)
        .style(|t: &Theme, status| style::checkbox(palette(t).text)(t, status))
        .on_toggle(Message::Hooks);
    let label = tr("Report to Inbox when waiting or done");
    let detail = tr("A separate key that can only report.");
    let row = setting_row(Some(check.into()), label, Some(detail.to_string()), Space::new());
    widgets::group(tr("Inbox"), vec![pressable(row, Some(Message::Hooks(!on)))], None)
}

fn template_label(template: RightsTemplate) -> &'static str {
    match template {
        RightsTemplate::ReadOnly => tr("Read only"),
        RightsTemplate::ReadAndWrite => tr("Read and write"),
        RightsTemplate::FullAccess => tr("Full access"),
    }
}

fn template_detail(template: RightsTemplate) -> &'static str {
    match template {
        RightsTemplate::ReadOnly => tr("Reads everything, changes nothing."),
        RightsTemplate::ReadAndWrite => tr("Reads and changes everything; deleting waits for your approval."),
        RightsTemplate::FullAccess => tr("Reads, changes and deletes everything without asking."),
    }
}

/// What to do now and where the entry went.
fn done_step(state: &State) -> Element<'_, Message> {
    let name = chosen_name(state);
    let place = state.choice.and_then(|choice| found(state, choice)).flatten();
    let written = match place {
        Some(found) if matches!(found.method, Method::Cli { .. }) => {
            trf("Added with the {command} command.", &[("command", &found.place)])
        }
        Some(found) => trf("Added to {file}.", &[("file", &found.place)]),
        None => String::new(),
    };
    let mut lines = column![
        typo::text(typo::BODY, trf("Restart {name} to use Torro Workplace.", &[("name", &name)])),
        typo::text(typo::BODY, written).style(theme::text_2),
    ]
    .spacing(space::M);
    match &state.hooks {
        Some(Ok(file)) => {
            let added = trf("Hooks added to {file}.", &[("file", file)]);
            lines = lines.push(typo::text(typo::BODY, added).style(theme::text_2));
            if state.choice == Some(Choice::Agent(Target::Codex)) {
                let trust = tr("Codex runs new hooks only after you trust them: type /hooks in Codex.");
                lines = lines.push(typo::text(typo::BODY, trust).style(theme::text_2));
            }
        }
        Some(Err(error)) => {
            let problem = trf("Inbox reports could not be set up: {error}", &[("error", error)]);
            let error_text = |t: &Theme| text::Style { color: Some(palette(t).error_text) };
            lines = lines.push(typo::text(typo::BODY, problem).style(error_text));
        }
        None => {}
    }
    let again = tr("Disconnect… on its page removes the entry again.");
    lines.push(typo::text(typo::BODY, again).style(theme::text_2)).into()
}

/// The key this once: why it is shown, the key in a field to select, and "Copy".
fn key_step(state: &State) -> Element<'_, Message> {
    let mut body = column![].spacing(space::L);
    if let Some(error) = &state.error {
        let problem = trf("The configuration could not be written: {error}", &[("error", error)]);
        body = body.push(
            typo::text(typo::BODY, problem).style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }),
        );
        body = body.push(
            typo::text(typo::BODY, tr("Put this key into its configuration yourself. It is shown only this once."))
                .style(theme::text_2),
        );
    } else {
        body = body.push(
            typo::text(
                typo::BODY,
                trf(
                    "Pass it to your program as {variable}. It is shown only this once; Torro Workplace keeps only a fingerprint of it.",
                    &[("variable", &TOKEN_VARIABLE)],
                ),
            )
            .style(theme::text_2),
        );
    }
    let key = state.key.as_ref().map_or("", IssuedKey::expose);
    let field = text_input("", key)
        .size(typo::BODY.size)
        .line_height(typo::BODY.line_height)
        .font(typo::BODY.font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        // Without an edit handler the field cannot take focus, so the key could not be selected.
        .on_input(|_| Message::KeepKey);
    let copy = match state.copied {
        true => widgets::text_button(Some(Icon::Check), tr("Copied"), style::button::secondary, Some(Message::CopyKey)),
        false => widgets::text_button(Some(Icon::Copy), tr("Copy"), style::button::secondary, Some(Message::CopyKey)),
    };
    body.push(row![field, copy].spacing(space::S).align_y(Alignment::Center)).into()
}

/// The error left (on the rights step), "Back", "Cancel" and the step's primary button right.
fn footer<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let error: Element<'a, Message> = match (&state.error, state.step) {
        (Some(error), Step::Rights) => typo::text(typo::BODY, error.as_str())
            .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) })
            .width(Fill)
            .into(),
        _ => blank::horizontal().into(),
    };
    let back = (state.step == Step::Rights)
        .then(|| widgets::text_button(None, tr("Back"), style::button::secondary, Some(Message::Back)));
    let cancel = matches!(state.step, Step::Client | Step::Rights)
        .then(|| widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Close)));
    let primary = |label, message| Some(widgets::text_button(None, label, style::button::primary, message));
    let next = match (state.step, state.choice) {
        (Step::Rights, Some(Choice::Script)) => {
            let ready = !state.tried || name_problem(&state.name, data).is_none();
            primary(tr("Create key"), ready.then_some(Message::Connect))
        }
        (Step::Rights, _) => primary(tr("Connect"), Some(Message::Connect)),
        (Step::Done | Step::Key, _) => primary(tr("Done"), Some(Message::Close)),
        (Step::Client | Step::Creating | Step::Writing | Step::CreatingHooks | Step::WritingHooks, _) => None,
    };
    row![error].push(back).push(cancel).push(next).spacing(space::S).align_y(Alignment::Center).into()
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
            Action::Send(Request::CreateClient { ref name, ref kind, template: RightsTemplate::ReadOnly, agent: None })
                if name == "Claude Code" && kind == "claude-code"
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

    #[test]
    fn a_script_needs_a_free_name_and_shows_its_key() {
        let data = Data { clients: vec![client("s1", "backup", "script")], ..Data::default() };
        let mut state = detected();
        let _ = update(&mut state, Message::Choose(Choice::Script), &data);
        assert!(name_problem("", &data).is_some());
        assert!(name_problem("CLI", &data).is_some(), "the app key's source");
        assert!(name_problem(" Backup ", &data).is_some(), "another script's name in any case");
        assert_eq!(name_problem("nightly", &data), None);

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
        let _ = update(&mut state, Message::Choose(Choice::Script), &data);
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
