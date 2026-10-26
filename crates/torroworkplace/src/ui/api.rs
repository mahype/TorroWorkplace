//! API (task S.35 in `docs/tasks/steuerzentrale.md`, concept in `docs/settings.md` › "API"):
//! programs that use Torro Workplace with a key – scripts and the like, the client kind `script` –
//! and the page of a client every section of programs shares ([`Page`]), which MCP (`mcp.rs`) uses
//! too.
//!
//! - **"API":** the switch "Allow API keys" (`settings.set_setting allow_api_keys`). Off, the core
//!   refuses every key of a script; only a person turns it on again (`Request::kept_for_people`).
//! - **"Keys":** a row per key – name, its rights in words ([`rights_summary`]), when it was last
//!   used –, which opens its page. "Create key" in the header opens the dialog of
//!   `agent_wizard.rs` on its path "Script" ([`agent_wizard::State::key`]): name, template, the key
//!   shown once. A client of a kind the suite does not know (from an older `clients.json`) is listed
//!   here too: the core checks it like a script.
//! - **A key's page** ([`Page`]): the rights editor of `torro-settings-ui` with the commands named
//!   as endpoints ([`Naming::Endpoints`]), its latest requests from the audit log (command and
//!   result, never contents), "Create new key" and "Revoke…", each behind a confirmation. A new key
//!   is shown once, with a copy button.
//!
//! The assistant, its bots and hook clients are on no page here ([`Home::of`]): the first two are
//! set in the assistant's window (task S.34), hooks on the page of their agent under MCP.
//!
//! Every change of the rights edits a draft, which the page shows at once; [`rights::SAVE_DELAY`]
//! after the last change it goes out as `clients.update_client` with the whole rights. The editor
//! keeps the draft; the shell hands every answer and reload to the page first ([`answered`],
//! [`loaded`]), so no control jumps back for a moment.
//!
//! Requests: `settings.set_setting` (the switch), `clients.create_client` ("Create key"),
//! `clients.update_client` (the rights), `clients.activity` (opening a page), `clients.new_key`,
//! `clients.remove_client` ("Revoke…"). The clients come from `clients.list_clients`, the
//! candidates of the selections from what the shell loaded into [`Data`]. Only the app key may send
//! `clients.*`; the control centre has it.

use chrono::{DateTime, Local, Utc};
use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{column, container, responsive, row};
use iced::{Alignment, Element, Fill, Length, Subscription};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, Status};
use torro_gui::{style, typo};
use torro_inbox::AgentKind;
use torro_ipc::api::{ASSISTANT_CLIENT, BOT_CLIENT, HOOK_CLIENT, MCP_CLIENT, SCRIPT_CLIENT, is_mcp_kind};
use torro_ipc::{ActivityEntry, ClientInfo, IssuedKey, Request, Response, SettingKey};
use torro_settings_ui::rights::{self, Naming, control_column, in_column, rights_summary};

use super::accounts::{on_axis, relative_time};
use super::{Action, Data, Header, agent_wizard, chevron, confirm, dialog, pressable, row_frame, setting_row};

/// The latest requests a client's page shows; the core keeps 50.
const ACTIVITY_SHOWN: usize = 10;

// ----- The page of a client, shared with MCP ----------------------------------------------------

/// The section a client is shown in: API for keys of programs, MCP for MCP clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Home {
    Api,
    Mcp,
}

impl Home {
    /// Where `client` is listed: MCP for the MCP kinds (`api::is_mcp_kind`), nowhere for the
    /// assistant, a bot and a hook client, API for a script and every kind the suite does not know
    /// (the core checks those like a script).
    pub fn of(client: &ClientInfo) -> Option<Home> {
        match client.kind.as_str() {
            ASSISTANT_CLIENT | BOT_CLIENT | HOOK_CLIENT => None,
            kind if is_mcp_kind(kind) => Some(Home::Mcp),
            _ => Some(Home::Api),
        }
    }

    /// How the rights editor names the commands here: endpoints for a key, tools for MCP.
    fn naming(self) -> Naming {
        match self {
            Home::Api => Naming::Endpoints,
            Home::Mcp => Naming::Tools,
        }
    }
}

/// The clients listed in `home`, in the core's order.
pub fn listed(data: &Data, home: Home) -> impl Iterator<Item = &ClientInfo> {
    data.clients.iter().filter(move |client| Home::of(client) == Some(home))
}

/// What a client's page holds: which client it shows, the rights editor's draft, the client's
/// latest requests, the open confirmation and a new key shown once. One per section ([`Home`]); a
/// page only opens and acts on clients of its own home.
#[derive(Debug)]
pub struct Page {
    home: Home,
    /// The client whose page is open (its `ClientInfo::id`); `None` shows the section's list.
    pub selected: Option<String>,
    /// The rights editor: its draft on the way to the core, the typed repository, the open areas.
    rights: rights::State,
    /// The client whose activity was asked for and not answered yet.
    asked: Option<String>,
    /// The latest requests of a client, as `clients.activity` answered for it.
    activity: Option<(String, Vec<ActivityEntry>)>,
    /// The confirmation that is open.
    dialog: Option<Dialog>,
    /// The client a new key was asked for (`clients.new_key`), until the core answers.
    renewing: Option<String>,
    /// A key the core just handed out, shown once until "Done".
    issued: Option<Issued>,
    /// The client whose removal was sent, until the core confirms it.
    removing: Option<String>,
}

/// The confirmations of a client's page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    /// "Create a new key?": the old key stops working.
    NewKey,
    /// "Revoke this key?" (API) or "Disconnect this client?" (MCP).
    Remove,
}

/// A key just handed out by `clients.new_key`. It lives only in this window's memory and only until
/// "Done": the core cannot tell it again.
#[derive(Debug)]
struct Issued {
    /// The client's name, for the dialog title.
    name: String,
    key: IssuedKey,
    /// "Copy" was pressed; the button says "Copied".
    copied: bool,
}

/// What the controls of a client's page report.
#[derive(Debug, Clone)]
pub enum PageMessage {
    /// Opens the page of a client.
    Open(String),
    /// Back from the page to the list.
    Back,
    /// A control of the rights editor, its clock included ([`Page::subscription`]).
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
    /// "Revoke…" / "Disconnect…", then its confirmation.
    AskRemove,
    Remove,
    /// "Cancel" or Escape in a confirmation.
    CloseDialog,
}

impl Page {
    /// The page of `home`'s clients, showing the list.
    pub fn new(home: Home) -> Page {
        Page {
            home,
            selected: None,
            rights: rights::State::default(),
            asked: None,
            activity: None,
            dialog: None,
            renewing: None,
            issued: None,
            removing: None,
        }
    }

    /// The client whose page is open, if the core still lists it and it belongs here.
    pub fn client<'a>(&self, data: &'a Data) -> Option<&'a ClientInfo> {
        let id = self.selected.as_deref()?;
        listed(data, self.home).find(|client| client.id == id)
    }

    /// Applies `message`. Opening a page asks for the client's latest requests
    /// (`clients.activity`); a new key and the removal go out only after their confirmation;
    /// the rights go through the editor ([`rights::update`]), saved [`rights::SAVE_DELAY`] later.
    pub fn update(&mut self, message: PageMessage, data: &Data) -> Action {
        match message {
            PageMessage::Open(id) => self.open(id),
            PageMessage::Back => {
                self.selected = None;
                self.dialog = None;
                // Leaving the page is a good moment to save; the clock would do it a moment later.
                from_editor(rights::save_now(&mut self.rights))
            }
            PageMessage::Rights(message) => {
                let client = self.client(data);
                from_editor(rights::update(&mut self.rights, message, client, &known(data)))
            }
            PageMessage::AskNewKey => self.ask(data, Dialog::NewKey),
            PageMessage::NewKey => {
                self.dialog = None;
                match self.client(data) {
                    Some(client) => {
                        self.renewing = Some(client.id.clone());
                        Action::Send(Request::NewClientKey { id: client.id.clone() })
                    }
                    None => Action::None,
                }
            }
            PageMessage::CopyKey => match self.issued.as_mut() {
                Some(issued) => {
                    issued.copied = true;
                    Action::Copy(issued.key.expose().to_owned())
                }
                None => Action::None,
            },
            PageMessage::CloseKey => {
                self.issued = None;
                Action::None
            }
            PageMessage::KeepKey => Action::None,
            PageMessage::AskRemove => self.ask(data, Dialog::Remove),
            PageMessage::Remove => {
                self.dialog = None;
                let Some(id) = self.client(data).map(|client| client.id.clone()) else { return Action::None };
                self.selected = None;
                // Its rights need not be saved any more.
                rights::forget(&mut self.rights, &id);
                self.remove(id)
            }
            PageMessage::CloseDialog => {
                self.dialog = None;
                Action::None
            }
        }
    }

    /// Sends `clients.remove_client` for `id` and waits for the core to confirm it ([`answered`]
    /// then names it). MCP also removes the hook clients of an agent that is gone this way.
    pub fn remove(&mut self, id: String) -> Action {
        self.removing = Some(id.clone());
        Action::Send(Request::RemoveClient { id })
    }

    /// Whether a removal is on its way.
    pub fn is_removing(&self) -> bool {
        self.removing.is_some()
    }

    /// Opens the page of the client `id`: its latest requests come with it.
    fn open(&mut self, id: String) -> Action {
        self.selected = Some(id.clone());
        self.dialog = None;
        rights::leave(&mut self.rights);
        self.activity = None;
        self.asked = Some(id.clone());
        Action::Send(Request::ClientActivity { id })
    }

    /// Opens `dialog` for the client on the page; without one there is nothing to confirm.
    fn ask(&mut self, data: &Data, dialog: Dialog) -> Action {
        self.dialog = self.client(data).map(|_| dialog);
        Action::None
    }

    /// The answer to a request the shell sent (it calls this for every answer, then reloads):
    ///
    /// - `ok` while the draft is on its way: saved, it goes with the next reload ([`Page::loaded`]);
    ///   an error then drops it, so the page shows what the core holds and the shell the error,
    /// - the activity asked for when the page opened,
    /// - the new key asked for with "Create new key" ([`Response::ClientKey`] of that client):
    ///   shown once; a new key of any other client is not this page's,
    /// - `ok` after the removal: returns the removed client's id, so the section can take it out of
    ///   an agent's configuration too; an error forgets the removal.
    pub fn answered(&mut self, result: &Result<Response, String>) -> Option<String> {
        let for_draft = rights::answered(&mut self.rights, result);
        if !for_draft && self.removing.is_some() {
            match result {
                Ok(Response::Ok) => return self.removing.take(),
                Err(_) => self.removing = None,
                Ok(_) => {}
            }
        }
        match result {
            Ok(Response::Activity { entries }) => {
                if let Some(id) = self.asked.take() {
                    self.activity = Some((id, entries.clone()));
                }
            }
            Ok(Response::ClientKey { client, key }) if self.renewing.as_deref() == Some(client.id.as_str()) => {
                self.renewing = None;
                self.issued = Some(Issued { name: client.name.clone(), key: key.clone(), copied: false });
            }
            Err(_) => self.renewing = None,
            _ => {}
        }
        None
    }

    /// The shell loaded [`Data`] anew: a saved draft is in it now.
    pub fn loaded(&mut self) {
        rights::loaded(&mut self.rights);
    }

    /// The editor's clock that saves a waiting draft; none while nothing waits, so the page costs
    /// nothing at rest.
    pub fn subscription(&self) -> Subscription<PageMessage> {
        rights::subscription(&self.rights).map(PageMessage::Rights)
    }

    /// What Escape means on the page right now: close the key dialog or the confirmation.
    pub fn escape(&self) -> Option<PageMessage> {
        if self.issued.is_some() {
            Some(PageMessage::CloseKey)
        } else if self.dialog.is_some() {
            Some(PageMessage::CloseDialog)
        } else {
            None
        }
    }

    /// The header of the page: the client's name with a back button to the list; `None` on the
    /// list.
    pub fn header(&self, data: &Data) -> Option<Header<'_, PageMessage>> {
        let client = self.client(data)?;
        Some(Header { sub_page: Some((client.name.clone(), PageMessage::Back)), actions: Vec::new() })
    }

    /// The page's dialog over the whole window: the new key, else a confirmation.
    pub fn overlay<'a>(&'a self, data: &'a Data) -> Option<Element<'a, PageMessage>> {
        if let Some(issued) = &self.issued {
            return Some(key_dialog(self.home, issued));
        }
        let client = self.client(data)?;
        let name: &dyn std::fmt::Display = &client.name;
        Some(match self.dialog? {
            Dialog::NewKey => confirm(
                tr("Create a new key?"),
                trf(
                    "The current key of {name} stops working at once. Put the new key into its configuration.",
                    &[("name", name)],
                ),
                tr("Create new key"),
                PageMessage::NewKey,
                PageMessage::CloseDialog,
            ),
            Dialog::Remove => {
                let (title, consequence, word) = remove_text(self.home, client);
                confirm(title, consequence, word, PageMessage::Remove, PageMessage::CloseDialog)
            }
        })
    }

    /// A client in the section's list: name, then what it is where the name does not say it (MCP),
    /// its rights in words and when it was last used; the row opens its page. While the section's
    /// switch is off (`refused`) the line starts with that the core refuses the client
    /// ([`refused_text`]), so the list does not look as if it worked.
    pub fn row<'a>(&self, client: &'a ClientInfo, now: DateTime<Utc>, refused: bool) -> Element<'a, PageMessage> {
        let mut parts: Vec<String> = refused.then(|| refused_text(self.home).to_string()).into_iter().collect();
        parts.extend(kind_word(self.home, client));
        parts.push(rights_summary(rights::shown(&self.rights, client), &client.kind));
        parts.push(last_used_text(client.last_used, now));
        let row = setting_row(None, client.name.as_str(), Some(parts.join(" · ")), chevron());
        pressable(row, Some(PageMessage::Open(client.id.clone())))
    }

    /// The top of a client's page: its name with what it is and when it was last used, then the
    /// rights editor ([`rights::groups`], the commands named after the section), the controls in
    /// `column` ([`control_column`]). The name is the first part, so a section can put a group of its
    /// own between it and the rights (MCP's "Notification Hub").
    pub fn head<'a>(
        &'a self,
        client: &'a ClientInfo,
        data: &'a Data,
        now: DateTime<Utc>,
        column: Option<f32>,
    ) -> Vec<Element<'a, PageMessage>> {
        let rights = rights::shown(&self.rights, client);
        let editor =
            rights::groups(&self.rights, rights, &client.kind, &known(data), self.home.naming(), tr("Access"), column);
        let mut parts = vec![heading(client.name.as_str(), about(self.home, client, now))];
        parts.extend(editor.into_iter().map(|group| group.map(PageMessage::Rights)));
        parts
    }

    /// The end of a client's page: its latest requests, its key with "Create new key", and
    /// "Revoke…" (API) or "Disconnect…" (MCP).
    pub fn foot<'a>(
        &'a self,
        client: &ClientInfo,
        now: DateTime<Utc>,
        column: Option<f32>,
    ) -> Vec<Element<'a, PageMessage>> {
        let away = match self.home {
            Home::Api => tr("Revoke…"),
            Home::Mcp => tr("Disconnect…"),
        };
        let away = widgets::text_button(None, away, style::button::danger, Some(PageMessage::AskRemove));
        vec![self.activity_group(client, now), key_group(self.home, column), on_axis(away)]
    }

    /// The latest requests: command, time and result. The log holds no contents, so neither does
    /// the page.
    fn activity_group<'a>(&'a self, client: &ClientInfo, now: DateTime<Utc>) -> Element<'a, PageMessage> {
        let title = tr("Recent activity");
        let entries = match &self.activity {
            Some((id, entries)) if *id == client.id => entries,
            _ => return quiet_group(title, tr("Loading…")),
        };
        if entries.is_empty() {
            return quiet_group(title, tr("No requests yet."));
        }
        let rows = entries.iter().take(ACTIVITY_SHOWN).map(|entry| activity_row(self.home, entry, now)).collect();
        let footer = tr("Only the command and its result are logged, never what it contained.");
        widgets::group(title, rows, Some(footer.to_string()))
    }
}

/// What the editor's selections offer: what the shell loaded into [`Data`].
fn known(data: &Data) -> rights::Known<'_> {
    rights::Known {
        calendars: &data.calendars,
        task_lists: &data.task_lists,
        sources: &data.sources,
        clients: &data.clients,
        accounts: &data.accounts,
        tools: &data.tools,
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

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// "Claude Code", "Other MCP client", "Script", "Claude Code hooks": what a client is.
pub fn kind_label(client: &ClientInfo) -> String {
    match client.kind.as_str() {
        "claude-desktop" => "Claude Desktop".to_string(),
        "cursor" => "Cursor".to_string(),
        SCRIPT_CLIENT => tr("Script").to_string(),
        MCP_CLIENT => tr("Other MCP client").to_string(),
        HOOK_CLIENT => match client.agent {
            Some(agent) => trf("{agent} hooks", &[("agent", &agent.label())]),
            None => tr("Hooks").to_string(),
        },
        kind => AgentKind::from_id(kind).map_or_else(|| kind.to_string(), |agent| agent.label().to_string()),
    }
}

/// What a client is, for the line under its name; `None` where that says nothing new: a key sits
/// in "Keys", and "Claude Code" needs no "Claude Code" below it.
fn kind_word(home: Home, client: &ClientInfo) -> Option<String> {
    let kind = kind_label(client);
    (home == Home::Mcp && kind != client.name).then_some(kind)
}

/// The start of a client's line in the list while the section's switch is off: the core refuses
/// every key of a script while "Allow API keys" is off, every MCP client while "Allow MCP" is.
pub fn refused_text(home: Home) -> &'static str {
    match home {
        Home::Api => tr("Refused while API keys are off"),
        Home::Mcp => tr("Refused while MCP is off"),
    }
}

/// A command of the audit log (its catalogue name) as the section names commands: the endpoint
/// (`calendar.list_events`) for a key, the MCP tool (`calendar_list_events`) for an MCP client –
/// the names the rights editor above it uses ([`Naming`]).
pub fn command_label(home: Home, command: &str) -> String {
    match home {
        Home::Api => command.to_string(),
        Home::Mcp => command.replace('.', "_"),
    }
}

/// "Used 5 min ago", or "Never used".
pub fn last_used_text(last_used: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    match last_used {
        Some(at) => trf("Used {when}", &[("when", &relative_time(at, now))]),
        None => tr("Never used").to_string(),
    }
}

/// The line under the name on a client's page: what it is, what kind where the name does not say
/// so, and when it was last used ("API key · Never used", "MCP client · Codex · Used 2 days ago").
pub fn about(home: Home, client: &ClientInfo, now: DateTime<Utc>) -> String {
    let what = match home {
        Home::Api => tr("API key"),
        Home::Mcp => tr("MCP client"),
    };
    let mut parts = vec![what.to_string()];
    parts.extend(kind_word(home, client));
    parts.push(last_used_text(client.last_used, now));
    parts.join(" · ")
}

/// Title, consequence and button word of the confirmation of "Revoke…" (a key: it only loses its
/// key) or "Disconnect…" (an MCP client: where "Connect client" set them up, its configuration
/// entry and its inbox hooks go too, [`crate::client_setup::Job::Disconnect`]).
fn remove_text(home: Home, client: &ClientInfo) -> (&'static str, String, &'static str) {
    let name: &dyn std::fmt::Display = &client.name;
    match home {
        Home::Api => (
            tr("Revoke this key?"),
            trf(
                "{name} loses access at once: its key stops working. Create a new key to give it access again.",
                &[("name", name)],
            ),
            tr("Revoke"),
        ),
        Home::Mcp => (
            tr("Disconnect this client?"),
            trf(
                "{name} loses access at once: its key stops working. Where Connect client set them up, its configuration entry and its Notification Hub hooks are removed too.",
                &[("name", name)],
            ),
            tr("Disconnect"),
        ),
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

// ----- Building blocks of the pages ---------------------------------------------------------

/// The name of a client's page with the line under it.
fn heading<'a, M: 'a>(name: &'a str, about: String) -> Element<'a, M> {
    let name = typo::text(typo::headline(), name).wrapping(Wrapping::None);
    let about = typo::text(typo::meta(), about).style(theme::text_2);
    column![container(name).width(Fill).clip(true), about].spacing(space::xs()).into()
}

/// A group title with a quiet sentence instead of rows (an empty group would be a bare frame): the
/// latest requests while they load, a section's list without clients.
pub fn quiet_group<'a, M: 'a>(title: &'a str, sentence: &'a str) -> Element<'a, M> {
    column![widgets::section_title(title), typo::text(typo::meta(), sentence).style(theme::text_3)]
        .spacing(space::s())
        .into()
}

/// A request: the command as `home` names it ([`command_label`]), below it the time in tabular
/// figures (so the times of the rows line up), and the result at the right. Laid out like a
/// `setting_row` with a detail, whose detail cannot be tabular; a command too long for its room is
/// cut with "…" and shown whole as tooltip.
fn activity_row<'a, M: 'a>(home: Home, entry: &'a ActivityEntry, now: DateTime<Utc>) -> Element<'a, M> {
    let (status, word) = outcome(&entry.result);
    let result = row![widgets::status_dot(status), typo::text(typo::meta(), word).style(theme::text_2)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    let lines = column![
        widgets::ellipsized(typo::body(), command_label(home, &entry.command), theme::text),
        widgets::ellipsized(typo::meta().tabular(), when(entry.time, now), theme::text_2),
    ];
    let text = lines.width(Fill);
    row_frame(row![text, result].spacing(space::l()).align_y(Alignment::Center), true)
}

/// The key: where it lives, and "Create new key".
fn key_group<'a>(home: Home, column: Option<f32>) -> Element<'a, PageMessage> {
    let create =
        widgets::text_button(None, tr("Create new key"), style::button::secondary, Some(PageMessage::AskNewKey));
    let detail = match home {
        Home::Api => tr("Only your program holds it"),
        Home::Mcp => tr("Only the agent's configuration holds it"),
    };
    let footer = tr("A new key replaces the old one at once.");
    let row = setting_row(None, tr("API key"), Some(detail.to_string()), in_column(create, column));
    widgets::group(tr("Key"), vec![row], Some(footer.to_string()))
}

/// The new key, shown this once: the key in a field to select, "Copy" and "Done". A click next to
/// the dialog does nothing, so a stray click does not lose the key.
fn key_dialog(home: Home, issued: &Issued) -> Element<'_, PageMessage> {
    let why = match home {
        Home::Api => tr(
            "Copy it into your program's configuration now. It is shown only this once; Torro Workplace keeps only a fingerprint of it.",
        ),
        Home::Mcp => tr(
            "Copy it into the agent's configuration now. It is shown only this once; Torro Workplace keeps only a fingerprint of it.",
        ),
    };
    let line = agent_wizard::copy_line(
        tr("Key"),
        issued.key.expose(),
        issued.copied,
        PageMessage::CopyKey,
        PageMessage::KeepKey,
    );
    let done = widgets::text_button(None, tr("Done"), style::button::primary, Some(PageMessage::CloseKey));
    let content = column![
        typo::text(typo::title(), trf("New key for {name}", &[("name", &issued.name)])),
        typo::text(typo::body(), why).style(theme::text_2),
        line,
        row![blank::horizontal(), done],
    ]
    .spacing(space::l());
    dialog(content, size::form_max(), PageMessage::KeepKey)
}

/// A group's last row that makes the first of what the group lists ("+ Create key…", "+ Connect
/// client…"): a ghost button on the content axis, as the groups of Accounts offer "+ Add …". It
/// stands in a group that is empty otherwise, so the group keeps its frame and footer.
pub fn add_row<'a, M: Clone + 'a>(label: &'a str, message: M) -> Element<'a, M> {
    let add = widgets::text_button(Some(Icon::Plus), label, style::button::ghost, Some(message));
    row_frame(on_axis(add), false)
}

/// The header's primary button: with its word, in narrow windows only its "+" icon, so the section
/// menu keeps its room.
pub fn primary<'a, M: Clone + 'a>(class: SizeClass, label: &'a str, message: M) -> Element<'a, M> {
    match class {
        SizeClass::Wide | SizeClass::Medium => {
            widgets::text_button(Some(Icon::Plus), label, style::button::primary, Some(message))
        }
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
            widgets::compact_button(Icon::Plus, label, style::button::primary, Some(message))
        }
    }
}

/// A switch of a section's way in ("Allow API keys", "Allow MCP"), with the line under it that says
/// what on and off mean.
pub fn allow_row<'a, M: Clone + 'a>(
    label: &'a str,
    on: bool,
    on_line: &'a str,
    off_line: &'a str,
    change: impl Fn(bool) -> M + 'a,
) -> Element<'a, M> {
    let detail = if on { on_line } else { off_line };
    setting_row(None, label, Some(detail.to_string()), torro_settings_ui::parts::switch(on, change))
}

// ----- The API section ----------------------------------------------------------------------

/// Page state of API.
#[derive(Debug)]
pub struct State {
    /// A key's page, or the list.
    pub page: Page,
    /// "Create key" while it is open.
    pub wizard: Option<agent_wizard::State>,
}

impl Default for State {
    fn default() -> Self {
        State { page: Page::new(Home::Api), wizard: None }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Page(PageMessage),
    /// The switch "Allow API keys".
    Allow(bool),
    /// "Create key" in the header.
    Create,
    Wizard(agent_wizard::Message),
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Page(message) => state.page.update(message, data),
        // Turning it on is the app key's only (`Request::kept_for_people`), which this window has.
        Message::Allow(on) => {
            Action::Send(Request::SetSetting { key: SettingKey::AllowApiKeys, value: serde_json::Value::Bool(on) })
        }
        Message::Create => {
            state.wizard = Some(agent_wizard::State::key());
            Action::None
        }
        Message::Wizard(message) => agent_wizard::update_in(&mut state.wizard, message, data),
    }
}

/// The answer to a request the shell sent: "Create key"'s own answers first
/// ([`agent_wizard::answered`]; then `Some`, and the shell shows no notice of its own), then the
/// page's ([`Page::answered`]). A revoked key leaves nothing else behind.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> Option<Action> {
    if let Some(next) = state.wizard.as_mut().and_then(|wizard| agent_wizard::answered(wizard, result)) {
        return Some(next);
    }
    let _ = state.page.answered(result);
    None
}

/// The shell loaded [`Data`] anew.
pub fn loaded(state: &mut State) {
    state.page.loaded();
}

pub fn subscription(state: &State) -> Subscription<Message> {
    state.page.subscription().map(Message::Page)
}

/// What Escape means here right now: close "Create key" (unless it is creating the key), the key
/// dialog or the confirmation.
pub fn escape(state: &State) -> Option<Message> {
    if let Some(wizard) = &state.wizard {
        return agent_wizard::closable(wizard).then_some(Message::Wizard(agent_wizard::Message::Close));
    }
    state.page.escape().map(Message::Page)
}

/// The header bar: on a key's page its name with a back button; on the list "Create key", the
/// window's one primary button.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Header<'a, Message> {
    match state.page.header(data) {
        Some(header) => header.map(Message::Page),
        None => Header { sub_page: None, actions: vec![primary(class, tr("Create key"), Message::Create)] },
    }
}

/// The section's dialog over the whole window: "Create key", else the page's.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(wizard) = &state.wizard {
        let content = agent_wizard::view(wizard, data).map(Message::Wizard);
        return Some(dialog(content, size::form_max(), Message::Wizard(agent_wizard::Message::Dismiss)));
    }
    state.page.overlay(data).map(|overlay| overlay.map(Message::Page))
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let now = Utc::now();
    match state.page.client(data) {
        Some(client) => responsive(move |size| detail(state, client, data, now, control_column(size.width)))
            .height(Length::Shrink)
            .into(),
        None => list(state, data, now),
    }
}

/// The list: the switch, then the keys.
fn list<'a>(state: &'a State, data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
    let allow = allow_row(
        tr("Allow API keys"),
        data.settings.allow_api_keys,
        tr("Scripts and programs with a key can use Torro Workplace"),
        tr("Every key of a script or program is refused"),
        Message::Allow,
    );
    let footer = tr("Programs reach Torro Workplace on this computer only. Only you can turn this on again.");
    let switch = widgets::group(tr("API"), vec![allow], Some(footer.to_string()));
    let refused = !data.settings.allow_api_keys;
    let mut rows: Vec<Element<'a, Message>> =
        listed(data, Home::Api).map(|client| state.page.row(client, now, refused).map(Message::Page)).collect();
    if rows.is_empty() {
        rows.push(add_row(tr("Create key…"), Message::Create));
    }
    let footer = tr("Scripts and other programs that use the API with their own key.");
    let keys = widgets::group(tr("Keys"), rows, Some(footer.to_string()));
    column![switch, keys].spacing(layout::section_gap()).into()
}

/// A key's page: the shared head and foot of [`Page`], the controls in `slot`.
fn detail<'a>(
    state: &'a State,
    client: &'a ClientInfo,
    data: &'a Data,
    now: DateTime<Utc>,
    slot: Option<f32>,
) -> Element<'a, Message> {
    let page = &state.page;
    let parts = page.head(client, data, now, slot).into_iter().chain(page.foot(client, now, slot));
    column(parts.map(|part| part.map(Message::Page))).spacing(layout::section_gap()).into()
}

#[cfg(test)]
pub(super) mod tests {
    use chrono::Duration as Age;
    use iced::time::Instant;
    use torro_inbox::{SourceInfo, SourceRef};
    use torro_ipc::api::{Area, Confirm, Level, Rights, RightsTemplate};
    use torro_ipc::{AccountInfo, Key};
    use torro_pim::model::GitHubFilter;
    use torro_pim::{AccountState, Collection, CollectionKind};
    use torro_settings_ui::rights::Pick;

    use super::*;

    pub fn client(id: &str, name: &str, kind: &str, rights: Rights) -> ClientInfo {
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

    /// Claude Code, its hooks, a script, the assistant and a bot, with calendars, lists, a source
    /// and accounts for the selections.
    pub fn data() -> Data {
        Data {
            clients: vec![
                client("c1", "Claude Code", "claude-code", RightsTemplate::ReadAndWrite.rights()),
                client("h1", "Claude Code hooks", HOOK_CLIENT, Rights::hook()),
                client("s1", "backup", "script", RightsTemplate::ReadOnly.rights()),
                client("a1", "Assistant", ASSISTANT_CLIENT, Rights::assistant()),
                client("b1", "Travel planner", BOT_CLIENT, Rights::none()),
            ],
            calendars: vec![
                collection("acc/work", "Work", CollectionKind::Events, &[]),
                collection("acc/home", "Home", CollectionKind::Events, &[]),
            ],
            task_lists: vec![
                collection("acc/todo", "Todo", CollectionKind::Todos, &[]),
                collection("github:anna/1", "Bugs", CollectionKind::Todos, &["is:open repo:anna/torro label:bug"]),
            ],
            sources: vec![SourceInfo { source: SourceRef::script("nightly"), open: 1, muted: false, health: None }],
            accounts: vec![
                account("nextcloud:anna@cloud.example.org", "nextcloud", "anna@cloud.example.org"),
                account("mail:anna@mailbox.org", "mail", "anna@mailbox.org"),
            ],
            ..Data::default()
        }
    }

    /// The API section with the page of `id` open.
    fn opened(id: &str, data: &Data) -> State {
        let mut state = State::default();
        let action = update(&mut state, Message::Page(PageMessage::Open(id.into())), data);
        assert!(matches!(action, Action::Send(Request::ClientActivity { id: ref asked }) if asked == id));
        state
    }

    /// Runs the clock past the delay and returns the request it sends.
    fn saved(state: &mut State, data: &Data) -> (String, Rights) {
        let later = Instant::now() + rights::SAVE_DELAY + rights::SAVE_DELAY;
        match update(state, Message::Page(PageMessage::Rights(rights::Message::Clock(later))), data) {
            Action::Send(Request::UpdateClient { id, rights }) => (id, rights),
            other => panic!("expected clients.update_client, got {other:?}"),
        }
    }

    fn editor(message: rights::Message) -> Message {
        Message::Page(PageMessage::Rights(message))
    }

    /// Task S.35: API lists scripts – and kinds the suite does not know, which the core checks like
    /// a script –, MCP the MCP clients; the assistant, bots and hooks are on no page.
    #[test]
    fn each_client_has_its_home() {
        let mut data = data();
        data.clients.push(client("o1", "older", "something-older", Rights::none()));
        data.clients.push(client("m1", "Zed", MCP_CLIENT, Rights::none()));
        let ids = |home| listed(&data, home).map(|client| client.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids(Home::Api), ["s1", "o1"]);
        assert_eq!(ids(Home::Mcp), ["c1", "m1"]);
        assert_eq!((Home::Api.naming(), Home::Mcp.naming()), (Naming::Endpoints, Naming::Tools));
        // A page opens only a client of its own home.
        let mut state = State::default();
        let _ = update(&mut state, Message::Page(PageMessage::Open("c1".into())), &data);
        assert!(state.page.client(&data).is_none(), "an MCP client has no page under API");
        let _ = update(&mut state, Message::Page(PageMessage::Open("a1".into())), &data);
        assert!(state.page.client(&data).is_none(), "nor has the assistant");
    }

    /// Task S.33 in its new place: a key's page edits through the rights editor – a level and a
    /// command's switch end in one `clients.update_client` with the whole rights.
    #[test]
    fn a_key_s_page_edits_through_the_rights_editor() {
        let data = data();
        let mut state = opened("s1", &data);
        let _ = update(&mut state, editor(rights::Message::Expand(Area::Calendar)), &data);
        assert_eq!(subscription(&state).units(), 0, "opening an area changes no rights");
        drop(view(&state, &data));
        let _ = update(&mut state, editor(rights::Message::Level(Area::Calendar, Level::Read)), &data);
        let _ = update(&mut state, editor(rights::Message::Command("calendar.list_events", false)), &data);
        let (id, rights) = saved(&mut state, &data);
        assert_eq!(id, "s1");
        assert_eq!(rights.area(Area::Calendar).level, Level::Read);
        assert!(rights.denied.iter().eq(["calendar.list_events"]), "{:?}", rights.denied);
    }

    #[test]
    fn back_saves_at_once_and_the_draft_shows_until_the_reload() {
        let data = data();
        let mut state = opened("s1", &data);
        let _ = update(&mut state, editor(rights::Message::Level(Area::Tasks, Level::Write)), &data);
        let client = &data.clients[2];
        assert_eq!(rights::shown(&state.page.rights, client).area(Area::Tasks).level, Level::Write, "at once");
        let action = update(&mut state, Message::Page(PageMessage::Back), &data);
        assert!(matches!(action, Action::Send(Request::UpdateClient { ref id, .. }) if id == "s1"));
        let _ = answered(&mut state, &Ok(Response::Ok));
        assert_eq!(rights::shown(&state.page.rights, client).area(Area::Tasks).level, Level::Write, "until reload");
        loaded(&mut state);
        assert_eq!(rights::shown(&state.page.rights, client).area(Area::Tasks).level, Level::Read, "then the core's");
    }

    #[test]
    fn a_failed_save_drops_the_draft_and_the_clock_runs_only_while_a_change_waits() {
        let data = data();
        let mut state = opened("s1", &data);
        assert_eq!(subscription(&state).units(), 0);
        let _ = update(&mut state, editor(rights::Message::Confirm(Confirm::Never)), &data);
        assert_eq!(subscription(&state).units(), 1);
        let _ = update(&mut state, editor(rights::Message::Level(Area::Tasks, Level::Write)), &data);
        let _ = saved(&mut state, &data);
        assert_eq!(subscription(&state).units(), 0);
        let _ = answered(&mut state, &Err("forbidden".into()));
        assert_eq!(rights::shown(&state.page.rights, &data.clients[2]).area(Area::Tasks).level, Level::Read);
    }

    #[test]
    fn a_new_key_is_confirmed_shown_once_and_copied() {
        let data = data();
        let mut state = opened("s1", &data);
        let page = |message| Message::Page(message);
        assert!(matches!(update(&mut state, page(PageMessage::AskNewKey), &data), Action::None));
        assert_eq!(state.page.dialog, Some(Dialog::NewKey));
        assert!(matches!(escape(&state), Some(Message::Page(PageMessage::CloseDialog))));
        let action = update(&mut state, page(PageMessage::NewKey), &data);
        assert!(matches!(action, Action::Send(Request::NewClientKey { ref id }) if id == "s1"));
        assert_eq!(state.page.dialog, None);

        // Another client's new key is not this page's to show.
        let other = IssuedKey::new(Key::new("torro_c1_00ff"));
        let _ = answered(&mut state, &Ok(Response::ClientKey { client: data.clients[0].clone(), key: other }));
        assert!(state.page.issued.is_none());
        let key = IssuedKey::new(Key::new("torro_s1_00ff"));
        let _ = answered(&mut state, &Ok(Response::ClientKey { client: data.clients[2].clone(), key }));
        assert!(overlay(&state, &data).is_some(), "the key dialog");
        assert!(matches!(update(&mut state, page(PageMessage::KeepKey), &data), Action::None));
        assert!(state.page.issued.is_some(), "a click next to it keeps the key");
        let copy = update(&mut state, page(PageMessage::CopyKey), &data);
        assert!(matches!(copy, Action::Copy(ref text) if text == "torro_s1_00ff"));
        assert!(matches!(escape(&state), Some(Message::Page(PageMessage::CloseKey))));
        let _ = update(&mut state, page(PageMessage::CloseKey), &data);
        assert!(state.page.issued.is_none() && overlay(&state, &data).is_none(), "gone for good");
    }

    #[test]
    fn revoke_is_confirmed_and_removes_the_key() {
        let data = data();
        let mut state = opened("s1", &data);
        let page = |message| Message::Page(message);
        let _ = update(&mut state, editor(rights::Message::Level(Area::Tasks, Level::Write)), &data);
        let _ = update(&mut state, page(PageMessage::AskRemove), &data);
        assert_eq!(state.page.dialog, Some(Dialog::Remove));
        assert!(overlay(&state, &data).is_some());
        let action = update(&mut state, page(PageMessage::Remove), &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "s1"));
        assert!(state.page.selected.is_none() && subscription(&state).units() == 0, "nothing left to save");
        assert!(answered(&mut state, &Ok(Response::Ok)).is_none(), "a key leaves nothing else behind");
        assert!(!state.page.is_removing());
        assert_eq!(remove_text(Home::Api, &data.clients[2]).0, tr("Revoke this key?"));
        assert_eq!(remove_text(Home::Mcp, &data.clients[0]).0, tr("Disconnect this client?"));
    }

    #[test]
    fn create_key_opens_the_dialog_and_the_switch_sends_the_setting() {
        let data = data();
        let mut state = State::default();
        assert_eq!(header(&state, &data, SizeClass::Wide).actions.len(), 1, "Create key");
        assert!(matches!(update(&mut state, Message::Create, &data), Action::None));
        assert!(state.wizard.is_some() && overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::Wizard(agent_wizard::Message::Close))));
        let _ = update(&mut state, Message::Wizard(agent_wizard::Message::Close), &data);
        assert!(state.wizard.is_none());
        for on in [false, true] {
            let action = update(&mut state, Message::Allow(on), &data);
            assert!(matches!(
                action,
                Action::Send(Request::SetSetting { key: SettingKey::AllowApiKeys, ref value }) if *value == serde_json::Value::Bool(on)
            ));
        }
        let detail = opened("s1", &data);
        assert!(header(&detail, &data, SizeClass::Narrow).actions.is_empty(), "a page has no primary button");
    }

    #[test]
    fn the_activity_belongs_to_the_client_it_was_asked_for() {
        let data = data();
        let mut state = opened("s1", &data);
        let entry = ActivityEntry {
            time: Utc::now(),
            client: Some("s1".into()),
            command: "calendar.list_events".into(),
            ids: Vec::new(),
            result: "ok".into(),
            approval: None,
        };
        let _ = answered(&mut state, &Ok(Response::Activity { entries: vec![entry] }));
        assert!(matches!(&state.page.activity, Some((id, entries)) if id == "s1" && entries.len() == 1));
        let _ = update(&mut state, Message::Page(PageMessage::Back), &data);
        let mut other = data.clone();
        other.clients.push(client("s2", "other", "script", Rights::none()));
        let _ = update(&mut state, Message::Page(PageMessage::Open("s2".into())), &other);
        assert!(state.page.activity.is_none(), "another client's activity is not shown");
    }

    /// #231: the activity names commands as the section does, and a switched-off section says so in
    /// every row of its list.
    #[test]
    fn activity_and_rows_speak_the_section_s_language() {
        assert_eq!(command_label(Home::Api, "calendar.list_events"), "calendar.list_events");
        assert_eq!(command_label(Home::Mcp, "calendar.list_events"), "calendar_list_events");
        assert_eq!(refused_text(Home::Api), tr("Refused while API keys are off"));
        assert_eq!(refused_text(Home::Mcp), tr("Refused while MCP is off"));
        let mut data = data();
        let state = State::default();
        let now = Utc::now();
        for allowed in [true, false] {
            data.settings.allow_api_keys = allowed;
            drop(view(&state, &data));
            drop(state.page.row(&data.clients[2], now, !allowed));
        }
        drop(view(&State::default(), &Data::default()));
    }

    #[test]
    fn kinds_and_last_use_in_words() {
        let data = data();
        assert_eq!(kind_label(&data.clients[0]), "Claude Code");
        assert_eq!(kind_label(&data.clients[1]), trf("{agent} hooks", &[("agent", &"Claude Code")]));
        assert_eq!(kind_label(&data.clients[2]), tr("Script"));
        assert_eq!(kind_label(&client("m1", "Zed", MCP_CLIENT, Rights::none())), tr("Other MCP client"));
        assert_eq!(kind_word(Home::Mcp, &data.clients[0]), None, "the name says it already");
        assert_eq!(kind_word(Home::Api, &data.clients[2]), None, "keys have a section of their own");
        let mut renamed = data.clients[0].clone();
        renamed.name = "Laptop agent".into();
        assert_eq!(kind_word(Home::Mcp, &renamed).as_deref(), Some("Claude Code"));
        let now = Utc::now();
        assert_eq!(last_used_text(None, now), tr("Never used"));
        let minutes = trf("{n} min ago", &[("n", &5)]);
        assert_eq!(last_used_text(Some(now - Age::minutes(5)), now), trf("Used {when}", &[("when", &minutes)]));
        let never = tr("Never used");
        assert_eq!(about(Home::Api, &data.clients[2], now), format!("{} · {never}", tr("API key")));
        assert_eq!(about(Home::Mcp, &renamed, now), format!("{} · Claude Code · {never}", tr("MCP client")));
    }

    #[test]
    fn outcomes_are_words_with_a_status_and_times_read_the_same_everywhere() {
        assert_eq!(outcome("ok"), (Status::Ok, tr("Done").to_string()));
        assert_eq!(outcome("pending").0, Status::Attention);
        assert_eq!(outcome("denied").0, Status::Inactive);
        assert_eq!(outcome("conflict"), (Status::Error, trf("Failed: {code}", &[("code", &"conflict")])));
        let now = Utc::now();
        assert_eq!(when(now, now), now.with_timezone(&Local).format("%H:%M").to_string());
        let old = now - Age::days(9);
        assert_eq!(when(old, now), old.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string());
        // Noon of the day before, not 24 hours back: on the day summer time ends that is still today.
        let yesterday = now
            .with_timezone(&Local)
            .date_naive()
            .pred_opt()
            .and_then(|day| day.and_hms_opt(12, 0, 0))
            .and_then(|noon| noon.and_local_timezone(Local).single())
            .expect("yesterday noon exists")
            .with_timezone(&Utc);
        let clock = yesterday.with_timezone(&Local).format("%H:%M").to_string();
        assert_eq!(when(yesterday, now), trf("yesterday {time}", &[("time", &clock)]));
    }

    #[test]
    fn the_section_builds_in_every_state() {
        let data = data();
        let mut state = State::default();
        drop(view(&state, &data));
        drop(view(&state, &Data::default()));
        state = opened("s1", &data);
        drop(view(&state, &data));
        for pick in Pick::ALL {
            let _ = update(&mut state, editor(rights::Message::All(pick, false)), &data);
        }
        drop(view(&state, &data));
        let _ = update(&mut state, Message::Page(PageMessage::AskRemove), &data);
        assert!(overlay(&state, &data).is_some());
    }
}
