//! MCP connections (task MA.7 in `docs/tasks/mcp-anbindungen.md`, issue #302): what the suite runs
//! for its clients beyond its own commands – its built-in tools and the MCP servers it is a client of.
//! The assistant's settings only point here since this task (`torro_settings_ui::assistant::tools`).
//!
//! ```text
//! Built-in tools     Make PDFs (switch) · whether the assistant may use it · Folder · Templates ›
//!   › PDF templates  Letter · Report · Note: paper, margins, font size, header, footer, page numbers, logo
//! Catalog            a row per entry of the core's catalog, with the status of an installed one
//! Your own servers   a row per server a person added, with its status
//! ```
//!
//! **Built-in tools** moved here unchanged from the assistant (task WZ.6): the switch
//! (`settings.set_setting tool_enabled`) says whether a tool exists on this computer, for every
//! client; what a client may use is its rights. The row "may use it" only reads the assistant's
//! client (`clients.list_clients`, loaded by the shell) and brings the assistant's window to the
//! front, where Access sets its rights ([`ClientTarget::request`]); nothing here writes a client.
//! Only a person switches a tool on and sets a path (point 9 of "Was nie nach außen geht" in
//! `docs/api.md`): this window has the app key, and folder and logo come from the system's choosers,
//! which only the shell opens ([`Action::Pick`] with a [`Chooser`], [`pick`]); it hands the path
//! back as [`Message::Picked`]. Every change of a template field sends `settings.set_setting
//! pdf_template` with the **whole** template as loaded, the one field changed ([`template_request`]):
//! the key replaces a template whole. Header and footer go out with Enter or "Save", when another
//! line is started, or when the templates' page is left – never on every key.
//!
//! **Catalog** and **Your own servers** list what `extensions.list_catalog` and
//! `extensions.list_extensions` answered ([`Data::catalog`], [`Data::extensions`], each a
//! [`Listing`]); a list that failed is a sentence with "Retry" in its group ([`Action::Reload`]).
//! The shell asks for the catalog only while this section is open (it looks for programs on disk
//! each time).
//!
//! **Installing** (task MA.8, `docs/tasks/mcp-anbindungen.md`, "Zustimmen"): a catalog entry with
//! nothing missing ([`installable`]) has "Install…", which opens the dialog ([`overlay`]): the
//! entry's tools with the access the catalog gives each, a hidden field per secret with the link
//! where to get one, and that the server runs as the person. "Install" sends
//! `extensions.install_extension`; the core starts the server once and stores it switched off with
//! its tools waiting. The dialog then shows the tools the server **reported** (the answer's
//! [`ExtensionInfo`]), and "Allow tools" sends `extensions.confirm_tools` with exactly that list's
//! fingerprint ([`Message::AllowTools`]). A list that changed meanwhile (`conflict`) is read again
//! (`extensions.get_extension`) and shown for another look.
//!
//! **A server's page** ([`Page::Server`]): its status in words with a dot and its switch
//! (`extensions.set_enabled`), its version with "Update" when the catalog has another one
//! (`extensions.update_extension`), its tools with access and state and "Allow tools" for those
//! that wait, its secrets as "Stored" or "Missing" with "Replace…" (`extensions.set_secret`), its
//! output (`extensions.read_logs`, read when the page opens, in a field to select and copy from),
//! and "Remove…" with a question first (`extensions.remove_extension`). Who may use it is set per
//! client (API, MCP, the assistant's Access), never here. The sidebar marks the section while a
//! server waits for its tools or stopped after errors ([`alert`]).
//!
//! Every request of this section goes out as [`Action::AskConnections`] with what it is for
//! ([`Asked`]), and its answer comes back here only ([`answered`]), with the core's code: the
//! dialog and the page show their outcome themselves. A secret is a [`Secret`] from the key press
//! on, so it never shows in `Debug`.
//!
//! Where the next task builds on this: MA.9 (a server of one's own) puts "Add server…" into the
//! actions of [`header`] on the list and its form into [`State`] and [`overlay`]; after its
//! `install_extension` it goes on as the catalog does, through [`Install`]'s step of the reported
//! tools ([`Step::Tools`], [`tools_dialog`]) and [`Message::AllowTools`].

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, svg, text, text_editor, toggler};
use iced::{Alignment, Element, Fill, Theme};

use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::{Paper, PdfTemplate, PdfTemplateId, ToolId};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Status as Dot};
use torro_gui::{style, typo};
use torro_ipc::api::{ASSISTANT_CLIENT, Access, Area};
use torro_ipc::extensions::{
    CatalogEntry, CrashReason, ExtensionInfo, ExtensionTool, InstallFrom, ServerSpec, Source, Status, ToolState,
};
use torro_ipc::tools::ToolInfo;
use torro_ipc::{Request, Response, Secret, SettingKey};
use torro_settings_ui::rights::{first_sentence, plain, tool_level};

use super::{
    Action, ClientTarget, Data, Header, chevron, confirm, dialog, field, notice, picker, pressable, row_frame,
    setting_row, switch,
};

/// The file endings the logo chooser offers, as `PdfTemplate::check` takes them.
pub const LOGO_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// The even margins the picker offers, in millimetres; a margin set otherwise through the API is
/// listed among them ([`margin_options`]).
const EVEN_MARGINS: [u8; 8] = [5, 10, 15, 20, 25, 30, 40, 50];

/// The output field of a server's page shows this many lines before it scrolls.
const OUTPUT_LINES: f32 = 12.0;

/// What the core answered to one list request of this section (`extensions.list_catalog`,
/// `extensions.list_extensions`): the entries, or why there are none. The shell fills it with each
/// load ([`Listing::from_result`]); the default is an empty list, as before the first answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing<T> {
    /// The core's answer, in its order.
    Entries(Vec<T>),
    /// The request failed (an older core answers `unsupported`, the socket broke): the text for a
    /// person, shown in the list's group with "Retry".
    Failed(String),
}

impl<T> Default for Listing<T> {
    fn default() -> Self {
        Listing::Entries(Vec::new())
    }
}

impl<T> Listing<T> {
    /// `Ok` as [`Listing::Entries`], `Err` as [`Listing::Failed`].
    pub fn from_result(result: Result<Vec<T>, String>) -> Self {
        match result {
            Ok(entries) => Listing::Entries(entries),
            Err(why) => Listing::Failed(why),
        }
    }

    /// The entries; none when the list failed.
    pub fn entries(&self) -> &[T] {
        match self {
            Listing::Entries(entries) => entries,
            Listing::Failed(_) => &[],
        }
    }
}

/// The page of the section that is shown.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Page {
    /// The three parts: built-in tools, catalog, own servers.
    #[default]
    List,
    /// "PDF templates", below the PDF tool's row "Templates": Letter, Report and Note.
    PdfTemplates,
    /// The page of the installed server with this id (task MA.8), below its row in the list.
    Server(String),
}

/// The section's state: the page shown, the header or footer line being typed, why the last
/// chosen path could not be used, the install dialog and what a server's page holds besides the
/// server's data.
#[derive(Debug, Default)]
pub struct State {
    pub page: Page,
    draft: Option<Draft>,
    /// Shown above the page until the next chooser opens.
    problem: Option<String>,
    /// The install dialog of a catalog entry while it is open.
    install: Option<Install>,
    /// What the page of a server shows besides its data; new with every page opened.
    server: ServerState,
}

/// The install dialog (task MA.8): first the form, then the tools the server reported.
#[derive(Debug)]
pub struct Install {
    /// The catalog entry ([`CatalogEntry::id`]); installed, the server has this id.
    entry: String,
    /// What is typed into the fields of the entry's secrets, by name: [`Secret`]s from the first
    /// key, so `Debug` never shows them. Sent with "Install", dropped with the dialog.
    secrets: BTreeMap<String, Secret>,
    /// "Install" was pressed once: an empty secret says so from now on, not before.
    tried: bool,
    step: Step,
    /// A request of the dialog is on its way: its button shows that and takes no second press.
    busy: bool,
    /// Why the last request of the dialog did not go through.
    problem: Option<Problem>,
}

impl Install {
    fn new(entry: String) -> Install {
        Install { entry, secrets: BTreeMap::new(), tried: false, step: Step::Form, busy: false, problem: None }
    }

    /// The id of the installed server whose tools the dialog shows; `None` on the form.
    fn installed(&self) -> Option<&str> {
        match &self.step {
            Step::Form => None,
            Step::Tools(server) => Some(server.id.as_str()),
        }
    }
}

/// The two steps of the install dialog.
#[derive(Debug)]
pub enum Step {
    /// The catalog's tools, the secrets' fields and "Install".
    Form,
    /// The server as the core installed it, switched off: the tools it reported, which "Allow
    /// tools" allows with this list's fingerprint. MA.9 comes here after its own install.
    Tools(Box<ExtensionInfo>),
}

/// Why a request of the dialog did not go through: what that means for the person, when the code
/// says more than the core's sentence, and the core's sentence (in the suite's language).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Problem {
    what: Option<&'static str>,
    why: String,
}

/// What a server's page holds besides the server's data, which comes with every reload.
#[derive(Debug, Default)]
struct ServerState {
    /// A request of the page on its way: its button shows that.
    waiting: Option<Waiting>,
    /// Why the last request of the page did not go through, above the page.
    problem: Option<String>,
    /// The server's output (`extensions.read_logs`).
    output: Output,
    /// The dialog that replaces or enters a secret.
    replacing: Option<Replacing>,
    /// "Remove…" asks first.
    removing: bool,
}

/// A request of a server's page on its way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Switch,
    Allow,
    Update,
    Remove,
}

/// The output of a server as the page has it.
#[derive(Debug, Default)]
enum Output {
    /// Asked for, no answer yet.
    #[default]
    Reading,
    /// What the core answered, in a field that can be selected but not changed.
    Lines(text_editor::Content),
    /// Why it could not be read.
    Failed(String),
}

/// The dialog that stores a new value for one secret of a server.
#[derive(Debug)]
struct Replacing {
    /// The variable or header it goes to.
    name: String,
    value: Secret,
    /// "Save" was pressed once: an empty field says so from now on.
    tried: bool,
    busy: bool,
    problem: Option<String>,
}

/// What a request of this section was for ([`Action::AskConnections`]), so its answer acts only for
/// it ([`answered`]): each carries the catalog entry or the server's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// `extensions.install_extension` of the dialog for this catalog entry.
    Install(String),
    /// `extensions.confirm_tools` of this server, from the dialog or its page.
    Allow(String),
    /// `extensions.get_extension` after the tools changed under the dialog.
    Refresh(String),
    /// `extensions.set_enabled` on the server's page.
    Switch(String),
    /// `extensions.set_secret` from the server's page.
    Secret(String),
    /// `extensions.read_logs` for the server's page.
    Logs(String),
    /// `extensions.update_extension` on the server's page.
    Update(String),
    /// `extensions.remove_extension` on the server's page.
    Remove(String),
}

/// A header or footer line being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Draft {
    template: PdfTemplateId,
    line: Line,
    text: String,
}

/// The two typed lines of a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Header,
    Footer,
}

impl Line {
    /// "Header", "Footer".
    fn label(self) -> &'static str {
        match self {
            Line::Header => tr("Header"),
            Line::Footer => tr("Footer"),
        }
    }

    /// This line of `template`.
    fn of(self, template: &PdfTemplate) -> &str {
        match self {
            Line::Header => &template.header,
            Line::Footer => &template.footer,
        }
    }
}

/// One of the system's choosers, which only the shell opens ([`Action::Pick`], [`pick`]): the path a
/// person chooses there comes back as [`Message::Picked`] with the same chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chooser {
    /// The folder new PDFs go to.
    Folder,
    /// The logo of a PDF template, a picture of [`LOGO_EXTENSIONS`].
    Logo(PdfTemplateId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The switch of a tool: on or off for every client (`tool_enabled`).
    Switch(ToolId, bool),
    /// "The assistant may not use it": the assistant's window, whose Access sets its rights.
    Access,
    /// "Choose…" at the folder or at a template's logo: the shell opens the system's chooser.
    Choose(Chooser),
    /// The chooser closed: the path chosen, `None` when it was cancelled.
    Picked(Chooser, Option<PathBuf>),
    /// "Use desktop": no folder of its own any more (`pdf_folder` `null`).
    UseDesktop,
    /// The row "Templates": the page "PDF templates".
    Templates,
    /// The back button of a page below the list, or Escape there.
    Back,
    /// A paper of the template's picker.
    Paper(PdfTemplateId, Paper),
    /// The margins of the template's picker, top, right, bottom, left.
    Margins(PdfTemplateId, [u8; 4]),
    /// A font size of the template's picker, in points.
    FontSize(PdfTemplateId, u8),
    /// What is typed into a header or footer.
    Type(PdfTemplateId, Line, String),
    /// Enter in the line being typed, or its "Save".
    SaveLine,
    /// Escape while a line is typed: back to the stored one.
    DiscardLine,
    /// "Page numbers" on or off.
    PageNumbers(PdfTemplateId, bool),
    /// "Remove" at a template's logo.
    RemoveLogo(PdfTemplateId),
    /// "Retry" in a group whose list failed: load everything again now.
    Retry,
    /// The row of an installed server: its page, which reads its output.
    Open(String),
    /// "Install…" at a catalog entry: the install dialog, only for an entry [`installable`] names.
    Install(String),
    /// What is typed into the dialog's field of the secret with this name.
    TypeSecret(String, Secret),
    /// "Install" in the dialog: `extensions.install_extension` with the secrets typed.
    SubmitInstall,
    /// "Allow tools" in the dialog or on a server's page: `extensions.confirm_tools` for the server
    /// with the fingerprint of the list that is shown ([`ExtensionInfo::fingerprint`]).
    AllowTools(String, String),
    /// "Cancel", "Not now", a click next to the dialog or Escape.
    CloseInstall,
    /// The switch on a server's page.
    Enable(String, bool),
    /// "Replace…" or "Enter…" at the secret with this name on a server's page.
    Replace(String),
    /// What is typed into the field of the secret being replaced.
    TypeReplacement(Secret),
    /// "Save" in the dialog of the secret.
    SubmitReplacement,
    /// "Cancel" there, a click next to it or Escape.
    CloseReplacement,
    /// "Refresh" at the output: read it again.
    ReadOutput,
    /// Selecting in the output field; edits are dropped, the field only shows.
    Output(text_editor::Action),
    /// "Copy" at the output: all of it to the clipboard.
    CopyOutput,
    /// "Update" on a server's page: the catalog's version (`extensions.update_extension`).
    Update(String),
    /// "Remove…" on a server's page: asks first.
    AskRemove,
    /// "Remove server" in the question.
    Remove,
    /// "Cancel" in the question, a click next to it or Escape.
    CancelRemove,
    /// A link of a catalog entry (its website, where to get a secret): the browser.
    Link(String),
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Switch(tool, on) => Action::Send(switch_request(tool, on)),
        Message::Access => Action::Send(ClientTarget::Assistant.request()),
        Message::Choose(chooser) => {
            state.problem = None;
            Action::Pick(chooser)
        }
        Message::Picked(_, None) => Action::None,
        Message::Picked(chooser, Some(path)) => {
            let request = match chooser {
                Chooser::Folder => folder_request(&path),
                Chooser::Logo(id) => logo_request(data, id, &path),
            };
            match request {
                Some(request) => Action::Send(request),
                None => {
                    state.problem = Some(tr("This path cannot be used.").to_string());
                    Action::None
                }
            }
        }
        Message::UseDesktop => {
            Action::Send(Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::Value::Null })
        }
        Message::Templates => {
            state.page = Page::PdfTemplates;
            Action::None
        }
        // Leaving the templates saves a line being typed, so nothing typed is dropped silently.
        Message::Back => {
            state.page = Page::List;
            state.server = ServerState::default();
            saved_draft(state, data)
        }
        Message::Paper(id, paper) => changed(data, id, |template| template.paper = paper),
        Message::Margins(id, margins) => changed(data, id, |template| template.margins_mm = margins),
        Message::FontSize(id, points) => changed(data, id, |template| template.font_size_pt = points),
        Message::PageNumbers(id, on) => changed(data, id, |template| template.page_numbers = on),
        Message::RemoveLogo(id) => changed(data, id, |template| template.logo = None),
        Message::Type(id, line, text) => {
            // Starting another line saves the one before, so nothing typed is dropped silently.
            let before = state.draft.take().filter(|draft| draft.template != id || draft.line != line);
            let text = text.chars().filter(|c| !c.is_control()).take(PdfTemplate::MAX_LINE_CHARS).collect();
            state.draft = Some(Draft { template: id, line, text });
            before.and_then(|draft| line_request(data, &draft)).map_or(Action::None, Action::Send)
        }
        Message::SaveLine => saved_draft(state, data),
        Message::DiscardLine => {
            state.draft = None;
            Action::None
        }
        Message::Retry => Action::Reload,
        Message::Open(id) => {
            state.page = Page::Server(id.clone());
            state.server = ServerState::default();
            logs_request(id)
        }
        Message::Install(entry) => {
            if installable(data, &entry).is_some() {
                state.install = Some(Install::new(entry));
            }
            Action::None
        }
        Message::TypeSecret(name, value) => {
            if let Some(install) = state.install.as_mut() {
                install.secrets.insert(name, value);
            }
            Action::None
        }
        Message::SubmitInstall => submit_install(state, data),
        Message::AllowTools(id, fingerprint) => {
            if let Some(install) = state.install.as_mut().filter(|install| install.installed() == Some(id.as_str())) {
                (install.busy, install.problem) = (true, None);
            }
            if state.page == Page::Server(id.clone()) {
                (state.server.waiting, state.server.problem) = (Some(Waiting::Allow), None);
            }
            ask(Asked::Allow(id.clone()), Request::ConfirmExtensionTools { id, fingerprint })
        }
        Message::CloseInstall => {
            state.install = None;
            Action::None
        }
        Message::Enable(id, enabled) => {
            (state.server.waiting, state.server.problem) = (Some(Waiting::Switch), None);
            ask(Asked::Switch(id.clone()), Request::SetExtensionEnabled { id, enabled })
        }
        Message::Replace(name) => {
            state.server.replacing =
                Some(Replacing { name, value: Secret::default(), tried: false, busy: false, problem: None });
            Action::None
        }
        Message::TypeReplacement(value) => {
            if let Some(replacing) = state.server.replacing.as_mut() {
                replacing.value = value;
            }
            Action::None
        }
        Message::SubmitReplacement => submit_replacement(state),
        Message::CloseReplacement => {
            state.server.replacing = None;
            Action::None
        }
        Message::ReadOutput => match &state.page {
            Page::Server(id) => {
                let id = id.clone();
                state.server.output = Output::Reading;
                logs_request(id)
            }
            _ => Action::None,
        },
        Message::Output(action) => {
            // The field only shows: selecting and copying pass, typing does not.
            if let Output::Lines(content) = &mut state.server.output
                && !action.is_edit()
            {
                content.perform(action);
            }
            Action::None
        }
        Message::CopyOutput => match &state.server.output {
            Output::Lines(content) => Action::Copy(content.text()),
            _ => Action::None,
        },
        Message::Update(id) => {
            (state.server.waiting, state.server.problem) = (Some(Waiting::Update), None);
            ask(Asked::Update(id.clone()), Request::UpdateExtension { id, server: None })
        }
        Message::AskRemove => {
            state.server.removing = true;
            Action::None
        }
        Message::CancelRemove => {
            state.server.removing = false;
            Action::None
        }
        Message::Remove => {
            state.server.removing = false;
            let Page::Server(id) = &state.page else { return Action::None };
            let id = id.clone();
            (state.server.waiting, state.server.problem) = (Some(Waiting::Remove), None);
            ask(Asked::Remove(id.clone()), Request::RemoveExtension { id })
        }
        Message::Link(url) => Action::OpenUrl(url),
    }
}

/// A request of this section, with what it is for.
fn ask(asked: Asked, request: Request) -> Action {
    Action::AskConnections(asked, request)
}

/// `extensions.read_logs` for the page of the server `id`.
fn logs_request(id: String) -> Action {
    ask(Asked::Logs(id.clone()), Request::ReadExtensionLogs { id })
}

/// "Install": nothing while a request is on its way; with a secret of the entry left empty, only
/// the dialog says so from now on ("Install" stays pressable, design skill, section 5); else
/// `extensions.install_extension` with every secret of the entry as typed.
fn submit_install(state: &mut State, data: &Data) -> Action {
    let Some(install) = state.install.as_mut().filter(|install| !install.busy && install.installed().is_none()) else {
        return Action::None;
    };
    let Some(entry) = data.catalog.entries().iter().find(|entry| entry.id == install.entry) else {
        return Action::None;
    };
    install.tried = true;
    let typed = |name: &str| install.secrets.get(name).filter(|secret| !secret.is_blank());
    if entry.secrets.iter().any(|secret| typed(&secret.name).is_none()) {
        return Action::None;
    }
    let secrets: BTreeMap<String, Secret> =
        entry.secrets.iter().filter_map(|secret| Some((secret.name.clone(), typed(&secret.name)?.clone()))).collect();
    (install.busy, install.problem) = (true, None);
    let from = InstallFrom::Catalog { entry: entry.id.clone(), secrets };
    ask(Asked::Install(entry.id.clone()), Request::InstallExtension { from })
}

/// "Save" in the dialog of a secret: nothing while it is on its way or without a server's page;
/// with an empty field, only the dialog says so; else `extensions.set_secret`.
fn submit_replacement(state: &mut State) -> Action {
    let Page::Server(id) = &state.page else { return Action::None };
    let Some(replacing) = state.server.replacing.as_mut().filter(|replacing| !replacing.busy) else {
        return Action::None;
    };
    replacing.tried = true;
    if replacing.value.is_blank() {
        return Action::None;
    }
    (replacing.busy, replacing.problem) = (true, None);
    let request =
        Request::SetExtensionSecret { id: id.clone(), name: replacing.name.clone(), value: replacing.value.clone() };
    ask(Asked::Secret(id.clone()), request)
}

/// What an answer says for this section.
enum Outcome<'r> {
    /// A server as the core has it now.
    Server(&'r ExtensionInfo),
    /// A server's output.
    Logs(&'r [String]),
    /// Done, nothing to show (`ok`).
    Done,
    /// Refused with the core's code, or no answer (`None`), and the sentence for the person.
    Refused(Option<&'r str>, String),
}

fn outcome(result: &Result<Response, String>) -> Outcome<'_> {
    match result {
        Ok(Response::Extension { extension }) => Outcome::Server(extension),
        Ok(Response::Logs { lines }) => Outcome::Logs(lines),
        Ok(Response::Error { code, message }) => Outcome::Refused(Some(code.as_str()), message.clone()),
        Ok(_) => Outcome::Done,
        Err(message) => Outcome::Refused(None, message.clone()),
    }
}

/// The core answered a request of this section ([`Action::AskConnections`]). It acts only where the
/// dialog or the page that asked is still open for the same entry or server; what it says shows
/// there. Returns what follows: after a list of tools that changed under the dialog
/// (`conflict`), that server read again; else nothing (the shell reloads either way).
pub fn answered(state: &mut State, asked: Asked, result: &Result<Response, String>) -> Action {
    let outcome = outcome(result);
    let removing = matches!(asked, Asked::Remove(_));
    match asked {
        // Only the form that waits for it takes the answer.
        Asked::Install(entry) => {
            let Some(install) = state
                .install
                .as_mut()
                .filter(|install| install.entry == entry && install.busy && install.installed().is_none())
            else {
                return Action::None;
            };
            install.busy = false;
            match outcome {
                Outcome::Server(server) => install.step = Step::Tools(Box::new(server.clone())),
                Outcome::Refused(code, why) => install.problem = Some(Problem { what: install_problem(code), why }),
                Outcome::Logs(_) | Outcome::Done => {}
            }
            Action::None
        }
        Asked::Allow(id) => {
            if state.page == Page::Server(id.clone()) {
                state.server.waiting = None;
            }
            let in_dialog = state.install.as_ref().is_some_and(|install| install.installed() == Some(id.as_str()));
            match outcome {
                Outcome::Refused(code, why) if in_dialog => {
                    if let Some(install) = state.install.as_mut() {
                        (install.busy, install.problem) = (false, Some(Problem { what: None, why }));
                    }
                    match code == Some("conflict") {
                        true => ask(Asked::Refresh(id.clone()), Request::GetExtension { id }),
                        false => Action::None,
                    }
                }
                Outcome::Refused(_, why) => {
                    if state.page == Page::Server(id) {
                        state.server.problem = Some(why);
                    }
                    Action::None
                }
                // Allowed: the dialog has done its job; the page shows the server as reloaded.
                _ => {
                    if in_dialog {
                        state.install = None;
                    }
                    Action::None
                }
            }
        }
        Asked::Refresh(id) => {
            if let (Some(install), Outcome::Server(server)) = (state.install.as_mut(), outcome)
                && install.installed() == Some(id.as_str())
            {
                install.step = Step::Tools(Box::new(server.clone()));
            }
            Action::None
        }
        Asked::Logs(id) => {
            if state.page == Page::Server(id) {
                state.server.output = match outcome {
                    Outcome::Logs(lines) => Output::Lines(text_editor::Content::with_text(&lines.join("\n"))),
                    Outcome::Refused(_, why) => Output::Failed(why),
                    Outcome::Server(_) | Outcome::Done => Output::Lines(text_editor::Content::new()),
                };
            }
            Action::None
        }
        Asked::Secret(id) => {
            if state.page == Page::Server(id)
                && let Some(replacing) = state.server.replacing.as_mut()
            {
                match outcome {
                    Outcome::Refused(_, why) => (replacing.busy, replacing.problem) = (false, Some(why)),
                    _ => state.server.replacing = None,
                }
            }
            Action::None
        }
        Asked::Switch(id) | Asked::Update(id) | Asked::Remove(id) => {
            if state.page != Page::Server(id) {
                return Action::None;
            }
            state.server.waiting = None;
            match outcome {
                Outcome::Refused(_, why) => state.server.problem = Some(why),
                // Removed: its page has nothing left to show.
                _ if removing => {
                    state.page = Page::List;
                    state.server = ServerState::default();
                }
                _ => {}
            }
            Action::None
        }
    }
}

/// What a refused install means for the person, by the core's code: something to install first
/// (`unsupported`, the core names it), the id taken (`conflict`), else that it did not install.
fn install_problem(code: Option<&str>) -> Option<&'static str> {
    match code {
        Some("unsupported") => Some(tr("Something it needs is missing on this computer.")),
        Some("conflict") => Some(tr("It is installed already.")),
        _ => Some(tr("It could not be installed.")),
    }
}

/// What Escape means here: close the open dialog, drop the line being typed, else leave the page
/// below the list. `None` on the list while nothing is open.
pub fn escape(state: &State) -> Option<Message> {
    if state.install.is_some() {
        return Some(Message::CloseInstall);
    }
    if state.server.replacing.is_some() {
        return Some(Message::CloseReplacement);
    }
    if state.server.removing {
        return Some(Message::CancelRemove);
    }
    match (&state.draft, &state.page) {
        (Some(_), _) => Some(Message::DiscardLine),
        (None, Page::PdfTemplates | Page::Server(_)) => Some(Message::Back),
        (None, Page::List) => None,
    }
}

/// The header bar: on a page below the list its name (the templates', a server's) with a back
/// button; the list has no actions yet ("Add server…" comes with task MA.9).
pub fn header<'a>(state: &'a State, data: &'a Data, _class: SizeClass) -> Header<'a, Message> {
    match &state.page {
        Page::PdfTemplates => {
            Header { sub_page: Some((tr("PDF templates").to_string(), Message::Back)), actions: vec![] }
        }
        Page::Server(id) => {
            let name = server_of(data, id).map_or_else(|| id.clone(), |server| server.name.clone());
            Header { sub_page: Some((name, Message::Back)), actions: vec![] }
        }
        Page::List => Header::default(),
    }
}

/// The dialog the section has open, laid over the whole window by the shell: installing, a secret
/// to replace, or the question before a server is removed.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(install) = &state.install {
        return install_dialog(install, data);
    }
    let Page::Server(id) = &state.page else { return None };
    let server = server_of(data, id)?;
    if let Some(replacing) = &state.server.replacing {
        return Some(replace_dialog(replacing, server));
    }
    state.server.removing.then(|| {
        confirm(
            tr("Remove this MCP server?"),
            trf(
                "{name} stops, and its settings, its secrets and what clients may do with it are deleted. You can install it again later.",
                &[("name", &server.name)],
            ),
            tr("Remove server"),
            Message::Remove,
            Message::CancelRemove,
        )
    })
}

/// The count after the section in the sidebar and the menu: the servers installed.
pub fn count(data: &Data) -> usize {
    data.extensions.entries().len()
}

/// Opens the system's chooser `chooser` and waits for it: the folder or picture chosen, `None` when
/// it was cancelled. Run by the shell for [`Action::Pick`], never in the demo.
pub async fn pick(chooser: Chooser) -> Option<PathBuf> {
    let dialog = rfd::AsyncFileDialog::new();
    let chosen = match chooser {
        Chooser::Folder => dialog.set_title(tr("Choose a folder for PDFs")).pick_folder().await,
        Chooser::Logo(_) => {
            dialog.set_title(tr("Choose a logo")).add_filter(tr("Pictures"), &LOGO_EXTENSIONS).pick_file().await
        }
    };
    chosen.map(|file| file.path().to_path_buf())
}

// ----- Requests ----------------------------------------------------------------------------------

/// `settings.set_setting tool_enabled` for `tool`: exactly one request per flip of its switch.
pub fn switch_request(tool: ToolId, on: bool) -> Request {
    Request::SetSetting {
        key: SettingKey::ToolEnabled,
        value: serde_json::json!({ "tool": tool.name(), "enabled": on }),
    }
}

/// `settings.set_setting pdf_template` with the whole template `id`: what every change of a field
/// sends, so the template is replaced by itself with that one field changed.
pub fn template_request(id: PdfTemplateId, template: &PdfTemplate) -> Request {
    Request::SetSetting { key: SettingKey::PdfTemplate, value: template.to_value(id) }
}

/// `settings.set_setting pdf_folder` with the folder the person chose. `None` for a path that is not
/// plain text (it could not be stored as the person sees it).
pub fn folder_request(folder: &Path) -> Option<Request> {
    let path = folder.to_str()?;
    Some(Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::Value::String(path.to_string()) })
}

/// `settings.set_setting pdf_template` with the template `id` as `data` holds it now and `logo` as
/// its logo. `None` for a path that is not plain text.
pub fn logo_request(data: &Data, id: PdfTemplateId, logo: &Path) -> Option<Request> {
    let logo = logo.to_str()?.to_string();
    let mut template = data.settings.tools.pdf.template(id).clone();
    template.logo = Some(logo);
    Some(template_request(id, &template))
}

/// The whole template `id` from `data` with `change` applied, as one request.
fn changed(data: &Data, id: PdfTemplateId, change: impl FnOnce(&mut PdfTemplate)) -> Action {
    let mut template = data.settings.tools.pdf.template(id).clone();
    change(&mut template);
    Action::Send(template_request(id, &template))
}

/// The line being typed, sent and let go; nothing when none is typed or it is what is stored.
fn saved_draft(state: &mut State, data: &Data) -> Action {
    state.draft.take().and_then(|draft| line_request(data, &draft)).map_or(Action::None, Action::Send)
}

/// The request for a typed line; `None` when it is what is stored already.
fn line_request(data: &Data, draft: &Draft) -> Option<Request> {
    let mut template = data.settings.tools.pdf.template(draft.template).clone();
    let stored = match draft.line {
        Line::Header => &mut template.header,
        Line::Footer => &mut template.footer,
    };
    if *stored == draft.text {
        return None;
    }
    stored.clone_from(&draft.text);
    Some(template_request(draft.template, &template))
}

// ----- Views -------------------------------------------------------------------------------------

/// The page: why a chosen path was not taken, then the list's three parts or the templates. In a
/// narrow window the buttons are icons with a tooltip, so the labels keep their room (design skill,
/// section 6).
pub fn view<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    let words = matches!(class, SizeClass::Wide | SizeClass::Medium);
    let body: Element<'a, Message> = match &state.page {
        Page::List => column![tools_group(data, words), catalog_group(data, words), own_group(data)]
            .spacing(layout::section_gap())
            .into(),
        Page::PdfTemplates => templates_page(state, data, words),
        Page::Server(id) => server_page(state, data, id, words),
    };
    column![].push(state.problem.clone().map(notice)).push(body).spacing(layout::section_gap()).into()
}

/// "Built-in tools": per tool its switch with what it does and whether the assistant may use it, for
/// the PDF tool the folder its files go to and the row to its templates.
fn tools_group<'a>(data: &'a Data, words: bool) -> Element<'a, Message> {
    let mut rows = Vec::new();
    for info in &data.tools {
        rows.push(switch_row(info));
        rows.push(access_row(data, info.id));
        if info.id == ToolId::Pdf {
            rows.push(folder_row(data, words));
            rows.push(templates_row());
        }
    }
    if rows.is_empty() {
        rows.push(setting_row(None, tr("No tools"), None, Space::new()));
    }
    let footer = tr(
        "A tool works only while it is on here and a client's rights allow it. Only you can switch it on and choose the folder and the logos.",
    );
    widgets::group(tr("Built-in tools"), rows, Some(footer.to_string()))
}

/// A tool's switch, with what it does below its name.
fn switch_row(info: &ToolInfo) -> Element<'_, Message> {
    let id = info.id;
    wrapping_row(switch_label(id), info.description.as_str(), switch(info.enabled, move |on| Message::Switch(id, on)))
}

/// A row whose line below the label wraps instead of being cut: what a tool does is a sentence a
/// narrow window must not cut off. With room for it on one line it is as high as a [`setting_row`]
/// with a detail (56); it grows with each further line.
fn wrapping_row<'a>(label: &'a str, detail: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let lines = column![
        widgets::ellipsized(typo::body(), label, theme::text),
        typo::text(typo::meta(), detail).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)
    ]
    .width(Fill);
    // A strut as high as a tall row inside its padding (56 − 2 × 8), so one line of detail gives
    // exactly the height of the rows around it.
    let strut = Space::new().height(size::header() + space::m() - space::l());
    let text = row![strut, lines].width(Fill).align_y(Alignment::Center);
    let content = row![text, control.into()].spacing(space::l()).align_y(Alignment::Center);
    container(content).width(Fill).padding([space::s(), space::l()]).into()
}

/// What the switch of a tool is called.
fn switch_label(id: ToolId) -> &'static str {
    match id {
        ToolId::Pdf => tr("Make PDFs"),
    }
}

/// Whether the assistant may use the tool: a green dot, or an orange one (the tool may be on, but
/// the assistant may not use it: something to do) and a row that brings the assistant's window to
/// the front, where Access chooses its rights – the arrow of another window.
fn access_row<'a>(data: &Data, id: ToolId) -> Element<'a, Message> {
    if assistant_may_use(data, id) {
        return setting_row(Some(widgets::status_dot(Dot::Ok)), tr("The assistant may use it"), None, Space::new());
    }
    let detail = tr("Allow it in the assistant under Access");
    let row = setting_row(
        Some(widgets::status_dot(Dot::Attention)),
        tr("The assistant may not use it"),
        Some(detail.to_string()),
        arrow(),
    );
    pressable(row, Some(Message::Access))
}

/// "Folder": where new PDFs go, the desktop without one; "Choose…" and, with one, "Use desktop".
fn folder_row<'a>(data: &'a Data, words: bool) -> Element<'a, Message> {
    let folder = data.settings.tools.pdf.folder.as_deref();
    let detail = folder.map_or_else(|| tr("Desktop").to_string(), str::to_string);
    let mut buttons = row![choose_button(words, Chooser::Folder)].spacing(space::s());
    if folder.is_some() {
        buttons = buttons.push(match words {
            true => widgets::text_button(None, tr("Use desktop"), style::button::ghost, Some(Message::UseDesktop)),
            // Not the ✕: nothing is deleted, the folder goes back to the desktop.
            false => widgets::icon_button(Icon::History, tr("Use desktop"), Some(Message::UseDesktop)),
        });
    }
    setting_row(None, tr("Folder"), Some(detail), buttons.align_y(Alignment::Center))
}

/// "Templates": the names of the PDF templates, and the page they are set on.
fn templates_row<'a>() -> Element<'a, Message> {
    let names: Vec<&str> = PdfTemplateId::ALL.into_iter().map(template_name).collect();
    let row = setting_row(None, tr("Templates"), Some(names.join(", ")), chevron());
    pressable(row, Some(Message::Templates))
}

/// "PDF templates": a group per template; the last one says below it what header and footer fill in.
fn templates_page<'a>(state: &'a State, data: &'a Data, words: bool) -> Element<'a, Message> {
    let last = PdfTemplateId::ALL.len() - 1;
    let groups =
        PdfTemplateId::ALL.into_iter().enumerate().map(|(at, id)| template_group(state, data, id, words, at == last));
    column(groups).spacing(layout::section_gap()).into()
}

/// One template: paper, margins, font size, header, footer, page numbers and logo.
fn template_group<'a>(
    state: &'a State,
    data: &'a Data,
    id: PdfTemplateId,
    words: bool,
    last: bool,
) -> Element<'a, Message> {
    let template = data.settings.tools.pdf.template(id);
    let paper = picker(PaperChoice::ALL.to_vec(), PaperChoice(template.paper), move |choice: PaperChoice| {
        Message::Paper(id, choice.0)
    });
    let margins = picker(margin_options(template.margins_mm), Margins(template.margins_mm), move |choice: Margins| {
        Message::Margins(id, choice.0)
    });
    let sizes: Vec<Points> = (PdfTemplate::MIN_FONT_PT..=PdfTemplate::MAX_FONT_PT).map(Points).collect();
    let font = picker(sizes, Points(template.font_size_pt), move |choice: Points| Message::FontSize(id, choice.0));
    let numbers = switch(template.page_numbers, move |on| Message::PageNumbers(id, on));
    let example = trf("Page {n} of {m}", &[("n", &1), ("m", &3)]);
    let rows = vec![
        setting_row(None, tr("Paper"), None, paper),
        setting_row(None, tr("Margins"), None, margins),
        setting_row(None, tr("Font size"), None, font),
        line_row(state, id, Line::Header, template, words),
        line_row(state, id, Line::Footer, template, words),
        setting_row(None, tr("Page numbers"), Some(example), numbers),
        logo_row(id, template, words),
    ];
    let footer = last.then(|| {
        tr("In header and footer, {title} becomes the title of the PDF and {date} the day it is made.").to_string()
    });
    widgets::group(template_name(id), rows, footer)
}

/// A header or footer: the field with what is stored or being typed, and "Save" – always there, so
/// the field keeps its width while typing; it can be pressed only while the line differs.
fn line_row<'a>(
    state: &'a State,
    id: PdfTemplateId,
    line: Line,
    template: &'a PdfTemplate,
    words: bool,
) -> Element<'a, Message> {
    let stored = line.of(template);
    let typing = state.draft.as_ref().filter(|draft| draft.template == id && draft.line == line);
    let value = typing.map_or(stored, |draft| draft.text.as_str());
    // Narrow, the field shares its row with the label and the check: only the example fits.
    let placeholder = if words { tr("For example {title} · {date}") } else { tr("{title} · {date}") };
    let input =
        field(placeholder, value).on_input(move |text| Message::Type(id, line, text)).on_submit(Message::SaveLine);
    let save = typing.is_some_and(|draft| draft.text != stored).then_some(Message::SaveLine);
    let button = match words {
        true => widgets::text_button(None, tr("Save"), style::button::secondary, save),
        false => widgets::icon_button(Icon::Check, tr("Save"), save),
    };
    let control = row![input, button].spacing(space::s()).align_y(Alignment::Center);
    setting_row(None, line.label(), None, control)
}

/// "Logo": the file's name or "No logo"; "Choose…" and, with one, "Remove".
fn logo_row<'a>(id: PdfTemplateId, template: &PdfTemplate, words: bool) -> Element<'a, Message> {
    let detail = template.logo.as_deref().map_or_else(|| tr("No logo").to_string(), file_name);
    let mut buttons = row![choose_button(words, Chooser::Logo(id))].spacing(space::s());
    if template.logo.is_some() {
        let remove = Message::RemoveLogo(id);
        buttons = buttons.push(match words {
            true => widgets::text_button(None, tr("Remove"), style::button::ghost, Some(remove)),
            false => widgets::icon_button(Icon::X, tr("Remove"), Some(remove)),
        });
    }
    setting_row(None, tr("Logo"), Some(detail), buttons.align_y(Alignment::Center))
}

/// "Choose…", with its word or as a folder icon.
fn choose_button<'a>(words: bool, chooser: Chooser) -> Element<'a, Message> {
    let message = Message::Choose(chooser);
    match words {
        true => widgets::text_button(None, tr("Choose…"), style::button::secondary, Some(message)),
        false => widgets::icon_button(Icon::Folder, tr("Choose…"), Some(message)),
    }
}

/// "Catalog": a row per entry of the core's catalog ([`catalog_row`]), then the installed catalog
/// servers whose entry the catalog no longer has. A failed list is a sentence with "Retry".
fn catalog_group(data: &Data, words: bool) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = match &data.catalog {
        Listing::Failed(why) => vec![failed_row(tr("The catalog could not be listed"), why)],
        Listing::Entries(entries) => entries.iter().map(|entry| catalog_row(data, entry, words)).collect(),
    };
    let known = |entry: &str| data.catalog.entries().iter().any(|listed| listed.id == entry);
    rows.extend(
        data.extensions
            .entries()
            .iter()
            .filter(|server| matches!(&server.source, Source::Catalog { entry, .. } if !known(entry)))
            .map(|server| server_row(server, tr("No longer in the catalog").to_string())),
    );
    if rows.is_empty() {
        rows.push(setting_row(None, tr("The catalog is empty"), None, Space::new()));
    }
    let footer = tr("MCP servers Torro Workplace knows, each in a fixed version. They run as you on this computer.");
    widgets::group(tr("Catalog"), rows, Some(footer.to_string()))
}

/// "Your own servers": a row per server a person added, with what runs and its status; without any,
/// one sentence. A failed list is a sentence with "Retry".
fn own_group(data: &Data) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = match &data.extensions {
        Listing::Failed(why) => vec![failed_row(tr("The MCP servers could not be listed"), why)],
        Listing::Entries(servers) => servers
            .iter()
            .filter(|server| server.source == Source::Custom)
            .map(|server| server_row(server, address(&server.server)))
            .collect(),
    };
    if rows.is_empty() {
        rows.push(setting_row(None, tr("No servers of your own yet"), None, Space::new()));
    }
    let footer = tr("Programs or addresses that speak MCP, added by you. They run as you on this computer.");
    widgets::group(tr("Your own servers"), rows, Some(footer.to_string()))
}

/// An installed server: its name, `detail` below it, its status at the end and the chevron to its
/// page; the whole row opens the page.
fn server_row<'a>(server: &'a ExtensionInfo, detail: String) -> Element<'a, Message> {
    let control = row![status_words(server), chevron()].spacing(space::s()).align_y(Alignment::Center);
    pressable(setting_row(None, server.name.as_str(), Some(detail), control), Some(Message::Open(server.id.clone())))
}

/// How a server stands, at the end of its row: a dot and a word, so it never rests on colour alone.
fn status_words<'a>(server: &ExtensionInfo) -> Element<'a, Message> {
    let (dot, word) = status_of(server);
    row![widgets::status_dot(dot), typo::text(typo::meta(), word).style(theme::text_2).wrapping(Wrapping::None)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// A catalog entry: its name (with "Experimental" where the publisher says so), what it does, who
/// makes it under which licence in which version, and what of it is missing on this computer, in the
/// attention colour with what to install. At the end its website, then the installed server's status
/// and the chevron to its page (the whole row opens it), or "Install…" when nothing is missing.
fn catalog_row<'a>(data: &'a Data, entry: &'a CatalogEntry, words: bool) -> Element<'a, Message> {
    let server = installed(data, &entry.id);
    let mut name = row![widgets::ellipsized(typo::body(), entry.name.as_str(), theme::text)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if entry.experimental {
        name = name.push(widgets::pill(None, tr("Experimental")));
    }
    let mut lines = column![
        name,
        typo::text(typo::meta(), entry.description.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
        widgets::ellipsized(typo::meta(), facts(entry), theme::text_3),
    ]
    .width(Fill);
    if server.is_none() && !entry.missing.is_empty() {
        let needs =
            typo::text(typo::meta(), missing_sentence(entry)).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
        let line = row![widgets::status_dot(Dot::Attention), needs].spacing(space::s()).align_y(Alignment::Center);
        lines = lines.push(container(line).padding([space::xs(), 0.0]));
    }
    let website = widgets::icon_button(Icon::ArrowUpRight, tr("Website"), Some(Message::Link(entry.url.clone())));
    let mut controls = row![website].spacing(space::s()).align_y(Alignment::Center);
    if let Some(server) = server {
        controls = controls.push(status_words(server)).push(chevron());
    } else if installable(data, &entry.id).is_some() {
        let install = Message::Install(entry.id.clone());
        controls = controls.push(match words {
            true => widgets::text_button(None, tr("Install…"), style::button::secondary, Some(install)),
            false => widgets::icon_button(Icon::Plus, tr("Install…"), Some(install)),
        });
    }
    let content = row![lines, controls].spacing(space::l()).align_y(Alignment::Center);
    let row = container(content).width(Fill).padding([space::m(), space::l()]);
    match server {
        Some(server) => pressable(row.into(), Some(Message::Open(server.id.clone()))),
        None => row.into(),
    }
}

/// "Model Context Protocol · MIT · version 2026.10.19": who makes an entry, its licence and the
/// version the catalog installs.
fn facts(entry: &CatalogEntry) -> String {
    trf(
        "{publisher} · {license} · version {version}",
        &[("publisher", &entry.publisher), ("license", &entry.license), ("version", &entry.version)],
    )
}

/// What a catalog entry misses, as the sentence to act on: "Needs Docker. Install it to use this
/// server."
fn missing_sentence(entry: &CatalogEntry) -> String {
    let needs: Vec<String> = entry.missing.iter().map(|what| what.needs()).collect();
    trf("{needs}. Install it to use this server.", &[("needs", &needs.join(" · "))])
}

// ----- A server's page ---------------------------------------------------------------------------

/// The page of the server `id`: a refusal of the page's last request, then its status, tools,
/// secrets (when it has any), output and "Remove…". A server no longer installed (removed elsewhere)
/// is one sentence.
fn server_page<'a>(state: &'a State, data: &'a Data, id: &str, words: bool) -> Element<'a, Message> {
    let Some(server) = server_of(data, id) else {
        let gone = setting_row(None, tr("This MCP server isn't installed any more"), None, Space::new());
        return widgets::group(tr("Status"), vec![gone], None);
    };
    let page = &state.server;
    let mut parts: Vec<Element<'a, Message>> = Vec::new();
    if let Some(problem) = &page.problem {
        parts.push(notice(problem.clone()));
    }
    parts.push(status_group(page, data, server, words));
    parts.push(tools_of_server(page, server, words));
    if !server.secrets.is_empty() {
        parts.push(secrets_group(server, words));
    }
    parts.push(output_group(page));
    parts.push(remove_group(page, words));
    column(parts).spacing(layout::section_gap()).into()
}

/// "Status": how the server stands with its switch, and where it comes from with "Update" when the
/// catalog has another version. Below it, who may use it: the clients' rights, set elsewhere.
fn status_group<'a>(
    page: &ServerState,
    data: &'a Data,
    server: &'a ExtensionInfo,
    words: bool,
) -> Element<'a, Message> {
    let (dot, word) = status_of(server);
    // A server whose tools were never allowed cannot be switched on: "Allow tools" does that.
    let never_allowed = !server.enabled && !server.tools.iter().any(|tool| tool.state == ToolState::Allowed);
    let id = server.id.clone();
    // Without a message the switch is drawn disabled; `switch` always takes one.
    let mut switch = toggler(server.enabled).size(style::CHECK_SIZE).style(style::toggler);
    if !never_allowed && page.waiting.is_none() {
        switch = switch.on_toggle(move |on| Message::Enable(id.clone(), on));
    }
    let rows = vec![
        status_row(widgets::status_dot(dot), word, status_detail(server, never_allowed), switch),
        source_row(page, data, server, words),
    ];
    let footer = tr("Who may use it: API, MCP and the assistant's Access.");
    widgets::group(tr("Status"), rows, Some(footer.to_string()))
}

/// A row with a mark, a label and a sentence below it that wraps instead of being cut ([`wrapping_row`]
/// with a mark): what a server's status means is a sentence a narrow window must not cut off.
fn status_row<'a>(
    mark: Element<'a, Message>,
    label: String,
    detail: Option<String>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut lines = column![widgets::ellipsized(typo::body(), label, theme::text)].width(Fill);
    if let Some(detail) = detail {
        lines = lines.push(typo::text(typo::meta(), detail).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    let strut = Space::new().height(size::header() + space::m() - space::l());
    let text = row![strut, mark, lines].spacing(space::m()).width(Fill).align_y(Alignment::Center);
    let content = row![text, control.into()].spacing(space::l()).align_y(Alignment::Center);
    container(content).width(Fill).padding([space::s(), space::l()]).into()
}

/// What the status of a server means and what a person can do, below its word; `None` where the
/// word says it all.
fn status_detail(server: &ExtensionInfo, never_allowed: bool) -> Option<String> {
    match &server.status {
        Status::Review if never_allowed => Some(tr("Allow its tools below to switch it on.").into()),
        Status::Review => Some(tr("Changed tools wait for you below; the others are offered as before.").into()),
        Status::Crashed { reason } => {
            Some(format!("{} {}", crash_reason(*reason), tr("Switch it off and on again to try again.")))
        }
        Status::Missing { what } => Some(trf("{needs}. Install it to use this server.", &[("needs", &what.needs())])),
        _ if !server.enabled => Some(tr("None of its tools is offered.").into()),
        Status::Stopped => Some(tr("It starts when one of its tools is called.").into()),
        Status::Running | Status::Starting => None,
    }
}

/// Why a server stopped, in the words the core uses for it.
fn crash_reason(reason: CrashReason) -> &'static str {
    match reason {
        CrashReason::Exited => tr("The MCP server stopped unexpectedly."),
        CrashReason::StartTimeout => tr("The MCP server didn't start in time."),
        CrashReason::SpawnFailed => tr("The MCP server's program couldn't be started."),
        CrashReason::Protocol => tr("The MCP server sent something Torro Workplace doesn't understand."),
    }
}

/// Where the server comes from: a catalog entry in its version, with "Update" when the catalog has
/// another one; one's own with its program or address (changing it comes with task MA.9).
fn source_row<'a>(page: &ServerState, data: &'a Data, server: &'a ExtensionInfo, words: bool) -> Element<'a, Message> {
    let Source::Catalog { entry, version } = &server.source else {
        let label = match server.server {
            ServerSpec::Stdio { .. } => tr("Program"),
            ServerSpec::Http { .. } => tr("Address"),
        };
        return setting_row(None, label, Some(address(&server.server)), Space::new());
    };
    let label = trf("Version {version}", &[("version", version)]);
    let Some(newer) = update_available(data, server) else {
        let from = data.catalog.entries().iter().find(|listed| &listed.id == entry);
        let detail = from.map_or_else(
            || tr("No longer in the catalog").to_string(),
            |entry| format!("{} · {}", entry.publisher, entry.license),
        );
        return setting_row(None, label, Some(detail), Space::new());
    };
    let detail = trf("Version {version} is in the catalog", &[("version", &newer)]);
    let button: Element<'a, Message> = match (page.waiting, words) {
        (Some(Waiting::Update), _) => widgets::busy_button(tr("Updating…"), style::button::secondary),
        (Some(_), true) => widgets::text_button(None, tr("Update"), style::button::secondary, None),
        (Some(_), false) => widgets::icon_button(Icon::RefreshCw, tr("Update"), None),
        (None, true) => {
            widgets::text_button(None, tr("Update"), style::button::secondary, Some(Message::Update(server.id.clone())))
        }
        (None, false) => widgets::icon_button(Icon::RefreshCw, tr("Update"), Some(Message::Update(server.id.clone()))),
    };
    setting_row(None, label, Some(detail), button)
}

/// "Tools": each with its state, the access a call needs and what the server says it does; then,
/// when any waits for a person, "Allow tools" for exactly this list. What the core noticed (tools it
/// does not offer) below.
fn tools_of_server<'a>(page: &ServerState, server: &'a ExtensionInfo, words: bool) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = server.tools.iter().map(|tool| tool_row(server, tool)).collect();
    if rows.is_empty() {
        rows.push(setting_row(None, tr("It reported no tools"), None, Space::new()));
    }
    if server.tools.iter().any(|tool| tool.state == ToolState::Review) {
        let busy = page.waiting == Some(Waiting::Allow);
        rows.push(setting_row(
            None,
            tr("Not offered until you allow them"),
            Some(tr("Allow the tools as they are listed here.").to_string()),
            allow_button(server, busy, page.waiting.is_some(), words),
        ));
    }
    let footer = (!server.notes.is_empty()).then(|| server.notes.join(" "));
    widgets::group(tr("Tools"), rows, footer)
}

/// "Allow tools" for `server` with the fingerprint of the list shown: the primary button of the page
/// or the dialog, "Allowing…" while it is on its way, not pressable while another request is or
/// without a list the core read.
fn allow_button<'a>(server: &ExtensionInfo, busy: bool, blocked: bool, words: bool) -> Element<'a, Message> {
    if busy {
        return widgets::busy_button(tr("Allowing…"), style::button::primary);
    }
    let press = server
        .fingerprint
        .clone()
        .filter(|_| !blocked)
        .map(|fingerprint| Message::AllowTools(server.id.clone(), fingerprint));
    match words {
        true => widgets::text_button(None, tr("Allow tools"), style::button::primary, press),
        false => widgets::compact_button(Icon::Check, tr("Allow tools"), style::button::primary, press),
    }
}

/// One tool of a server: a dot and its name, below it its state, the access it needs and the first
/// sentence of its description – the server's text, shown as data.
fn tool_row<'a>(server: &ExtensionInfo, tool: &'a ExtensionTool) -> Element<'a, Message> {
    let (dot, state) = tool_state(server, tool);
    let level = widgets::level_label(tool_level(tool.access));
    let said = plain(first_sentence(tool.description.trim()));
    let mut detail = vec![state, level.to_string()];
    if !said.is_empty() {
        detail.push(said);
    }
    setting_row(Some(widgets::status_dot(dot)), tool.name.as_str(), Some(detail.join(" · ")), Space::new())
}

/// The state of a tool in a dot and a word. A tool waiting for a person is "Not allowed yet" while
/// none of the server's tools was ever allowed (just installed), else it is new or changed since the
/// person allowed the list.
fn tool_state(server: &ExtensionInfo, tool: &ExtensionTool) -> (Dot, String) {
    match tool.state {
        ToolState::Allowed => (Dot::Ok, tr("Allowed").to_string()),
        ToolState::Review if server.tools.iter().any(|tool| tool.state == ToolState::Allowed) => {
            (Dot::Attention, tr("Changed since you allowed it").to_string())
        }
        ToolState::Review => (Dot::Attention, tr("Not allowed yet").to_string()),
        ToolState::Unsupported => (Dot::Inactive, tr("Not offered").to_string()),
    }
}

/// "Secrets": each as "Stored" or "Missing", never its value, with "Replace…" or "Enter…".
fn secrets_group<'a>(server: &'a ExtensionInfo, words: bool) -> Element<'a, Message> {
    let rows = server
        .secrets
        .iter()
        .map(|secret| {
            let (dot, word, action) = match secret.stored {
                true => (Dot::Ok, tr("Stored"), tr("Replace…")),
                false => (Dot::Attention, tr("Missing"), tr("Enter…")),
            };
            let press = Some(Message::Replace(secret.name.clone()));
            let button = match words {
                true => widgets::text_button(None, action, style::button::secondary, press),
                false => widgets::icon_button(Icon::SquarePen, action, press),
            };
            setting_row(Some(widgets::status_dot(dot)), secret.name.as_str(), Some(word.to_string()), button)
        })
        .collect();
    let footer = tr("Secrets stay in the system keychain. The server gets them when it starts.");
    widgets::group(tr("Secrets"), rows, Some(footer.to_string()))
}

/// "Output": what the server last wrote, in a field to select and copy from, with "Refresh" and
/// "Copy" at the heading.
fn output_group(page: &ServerState) -> Element<'_, Message> {
    let copy = matches!(&page.output, Output::Lines(content) if !content.text().trim().is_empty());
    let actions = vec![
        widgets::icon_button(Icon::RefreshCw, tr("Refresh"), Some(Message::ReadOutput)),
        widgets::icon_button(Icon::Copy, tr("Copy"), copy.then_some(Message::CopyOutput)),
    ];
    let body: Element<'_, Message> = match &page.output {
        Output::Reading => one_sentence(tr("Reading…")),
        Output::Failed(why) => one_sentence(why.as_str()),
        Output::Lines(content) if content.text().trim().is_empty() => one_sentence(tr("It wrote nothing yet.")),
        Output::Lines(content) => text_editor(content)
            .on_action(Message::Output)
            .font(typo::meta().font)
            .size(typo::meta().size)
            .line_height(typo::meta().line_height)
            .padding(style::INPUT_PADDING)
            .wrapping(Wrapping::WordOrGlyph)
            .height(output_height())
            .style(style::text_editor)
            .into(),
    };
    let footer = tr("The last lines it wrote, kept in memory only. Agents never see them.");
    column![widgets::group_heading(tr("Output"), actions), body, typo::text(typo::meta(), footer).style(theme::text_2),]
        .spacing(layout::item_gap())
        .into()
}

/// A sentence in a group's frame, where a field would be.
fn one_sentence<'a>(sentence: &'a str) -> Element<'a, Message> {
    let line = typo::text(typo::body(), sentence).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
    container(row_frame(line, false)).width(Fill).style(style::container::group).into()
}

/// The room of the output field: [`OUTPUT_LINES`] of `META` and the padding of a field.
fn output_height() -> f32 {
    OUTPUT_LINES * typo::line_height(typo::meta()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// "Remove": what removing does, and "Remove…", which asks first.
fn remove_group<'a>(page: &ServerState, words: bool) -> Element<'a, Message> {
    let button = match (page.waiting, words) {
        (Some(Waiting::Remove), _) => widgets::busy_button(tr("Removing…"), style::button::danger),
        (waiting, true) => widgets::text_button(
            None,
            tr("Remove…"),
            style::button::danger,
            waiting.is_none().then_some(Message::AskRemove),
        ),
        (waiting, false) => {
            widgets::icon_button(Icon::Trash2, tr("Remove…"), waiting.is_none().then_some(Message::AskRemove))
        }
    };
    let row = setting_row(
        None,
        tr("Remove this MCP server"),
        Some(tr("Stops it and deletes its settings, secrets and rights.").to_string()),
        button,
    );
    widgets::group(tr("Remove"), vec![row], None)
}

// ----- Dialogs -----------------------------------------------------------------------------------

/// The install dialog: the form of a catalog entry, or the tools the installed server reported.
/// `None` when the entry left the catalog meanwhile.
fn install_dialog<'a>(install: &'a Install, data: &'a Data) -> Option<Element<'a, Message>> {
    let entry = data.catalog.entries().iter().find(|entry| entry.id == install.entry)?;
    let content = match &install.step {
        Step::Form => form_dialog(install, entry),
        Step::Tools(server) => tools_dialog(install, entry, server),
    };
    Some(dialog(content, widgets::DIALOG_WIDTH, Message::CloseInstall))
}

/// The head of the install dialog: `title`, then who makes the entry in which version, and
/// "Experimental".
fn dialog_head<'a>(title: String, entry: &'a CatalogEntry) -> Element<'a, Message> {
    let mut facts = row![widgets::ellipsized(typo::meta(), facts(entry), theme::text_3)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if entry.experimental {
        facts = facts.push(widgets::pill(None, tr("Experimental")));
    }
    let words = column![typo::text(typo::title(), title), facts].spacing(space::xs()).width(Fill);
    let close = widgets::icon_button(Icon::X, tr("Close"), Some(Message::CloseInstall));
    row![words, close].spacing(space::m()).align_y(Alignment::Start).into()
}

/// The form: what the entry does, that it runs as the person, a hidden field per secret with where
/// to get one, and its tools with the access each needs; "Install" sends it.
fn form_dialog<'a>(install: &'a Install, entry: &'a CatalogEntry) -> Element<'a, Message> {
    let head = dialog_head(trf("Install {name}", &[("name", &entry.name)]), entry);
    let mut body = column![
        typo::text(typo::body(), entry.description.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
        runs_as_you(),
    ]
    .spacing(space::l());
    if !entry.secrets.is_empty() {
        let rows = entry.secrets.iter().map(|secret| secret_field(install, secret)).collect();
        let footer = tr("Kept in the system keychain; only this server gets it, when it starts.");
        body = body.push(widgets::group(tr("Secrets"), rows, Some(footer.to_string())));
    }
    let tools = entry.tools.iter().map(|tool| {
        let level = widgets::level_label(tool_level(tool.access));
        setting_row(None, tool.name.as_str(), Some(level.to_string()), Space::new())
    });
    let footer = tr("What a call of each tool needs. None is offered until you allow the tools it reports.");
    body = body.push(widgets::group(tr("Tools"), tools.collect(), Some(footer.to_string())));
    let install_button = match install.busy {
        true => widgets::busy_button(tr("Installing…"), style::button::primary),
        false => widgets::text_button(None, tr("Install"), style::button::primary, Some(Message::SubmitInstall)),
    };
    let wait = install.busy.then(|| tr("The first start can take a few minutes while the program is downloaded."));
    let foot = dialog_foot(install.problem.as_ref(), wait, tr("Cancel"), install_button);
    torro_gui::dialog::frame(head, body, foot, space::xl())
}

/// "It runs as you on this computer": the sentence every install shows, with the info icon.
fn runs_as_you<'a>() -> Element<'a, Message> {
    let info = icon::icon(Icon::Info, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let sentence = tr("It runs as you on this computer and can reach what you can.");
    row![info, typo::text(typo::body(), sentence).wrapping(Wrapping::WordOrGlyph)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// The field of one secret of the entry: its name, what it is, the hidden field and the link where
/// to get one; after the first "Install", a sentence when it is empty.
fn secret_field<'a>(install: &'a Install, secret: &'a torro_ipc::extensions::CatalogSecret) -> Element<'a, Message> {
    let typed = install.secrets.get(&secret.name);
    let name = secret.name.clone();
    let input = field("", typed.map_or("", Secret::expose))
        .secure(true)
        .on_input(move |value| Message::TypeSecret(name.clone(), Secret::new(value)))
        .on_submit(Message::SubmitInstall);
    let mut line = row![input].spacing(space::s()).align_y(Alignment::Center);
    if let Some(url) = &secret.url {
        line = line.push(widgets::text_button(
            Some(Icon::ArrowUpRight),
            tr("Get one"),
            style::button::ghost,
            Some(Message::Link(url.clone())),
        ));
    }
    let mut lines = column![
        widgets::ellipsized(typo::body(), secret.name.as_str(), theme::text),
        typo::text(typo::meta(), secret.hint.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
        line,
    ]
    .spacing(space::xs());
    if install.tried && typed.is_none_or(Secret::is_blank) {
        lines = lines.push(error_text(tr("Enter it to install the server.")));
    }
    container(lines).width(Fill).padding([space::m(), space::l()]).into()
}

/// The tools the installed server reported, each with its state, access and what it says it does,
/// and "Allow tools" for exactly this list; "Not now" leaves it installed and switched off, its
/// page has the same button. Who may use it is the clients' rights.
pub fn tools_dialog<'a>(
    install: &'a Install,
    entry: &'a CatalogEntry,
    server: &'a ExtensionInfo,
) -> Element<'a, Message> {
    let head = dialog_head(trf("Allow the tools of {name}?", &[("name", &server.name)]), entry);
    let mut rows: Vec<Element<'a, Message>> = server.tools.iter().map(|tool| tool_row(server, tool)).collect();
    if rows.is_empty() {
        rows.push(setting_row(None, tr("It reported no tools"), None, Space::new()));
    }
    let footer = (!server.notes.is_empty()).then(|| server.notes.join(" "));
    let body = column![
        typo::text(typo::body(), tr("It started and reports these tools. None is offered until you allow them."))
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph),
        widgets::group(tr("Tools"), rows, footer),
        typo::text(typo::meta(), tr("Who may use it: API, MCP and the assistant's Access.")).style(theme::text_2),
    ]
    .spacing(space::l());
    let allow = allow_button(server, install.busy, false, true);
    let foot = dialog_foot(install.problem.as_ref(), None, tr("Not now"), allow);
    torro_gui::dialog::frame(head, body, foot, space::xl())
}

/// The foot of the install dialog: why the last request failed (what it means, then the core's
/// sentence), a quiet note while it waits, then `cancel` and the primary button at the right.
fn dialog_foot<'a>(
    problem: Option<&'a Problem>,
    wait: Option<&'a str>,
    cancel: &'a str,
    primary: Element<'a, Message>,
) -> Element<'a, Message> {
    let mut foot = column![].spacing(space::s());
    if let Some(problem) = problem {
        let mut said = column![].spacing(space::xs());
        if let Some(what) = problem.what {
            said = said.push(error_text(what));
        }
        said = said
            .push(typo::text(typo::meta(), problem.why.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
        foot = foot.push(said);
    }
    if let Some(wait) = wait {
        foot = foot.push(typo::text(typo::meta(), wait).style(theme::text_3).wrapping(Wrapping::WordOrGlyph));
    }
    let buttons = row![
        Space::new().width(Fill),
        widgets::text_button(None, cancel, style::button::secondary, Some(Message::CloseInstall)),
        primary,
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    foot.push(buttons).into()
}

/// The dialog of one secret of the server: the hidden field, a sentence when it is empty after the
/// first "Save" or the core refused, "Cancel" and "Save".
fn replace_dialog<'a>(replacing: &'a Replacing, server: &'a ExtensionInfo) -> Element<'a, Message> {
    let stored = server.secrets.iter().any(|secret| secret.name == replacing.name && secret.stored);
    let title = match stored {
        true => trf("Replace {name}", &[("name", &replacing.name)]),
        false => trf("Enter {name}", &[("name", &replacing.name)]),
    };
    let input = field("", replacing.value.expose())
        .secure(true)
        .on_input(|value| Message::TypeReplacement(Secret::new(value)))
        .on_submit(Message::SubmitReplacement);
    let mut body = column![
        typo::text(typo::title(), title),
        typo::text(typo::body(), trf("{name} gets it when it starts next.", &[("name", &server.name)]))
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph),
        widgets::group(replacing.name.as_str(), vec![row_frame(input, false)], None),
    ]
    .spacing(space::l());
    if replacing.tried && replacing.value.is_blank() {
        body = body.push(error_text(tr("Enter the new value.")));
    }
    if let Some(problem) = &replacing.problem {
        body = body.push(error_text(problem.as_str()));
    }
    let save = match replacing.busy {
        true => widgets::busy_button(tr("Saving…"), style::button::primary),
        false => widgets::text_button(None, tr("Save"), style::button::primary, Some(Message::SubmitReplacement)),
    };
    let buttons = row![
        Space::new().width(Fill),
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::CloseReplacement)),
        save,
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    dialog(body.push(buttons), widgets::POPOVER_WIDTH, Message::CloseReplacement)
}

/// A sentence in the error colour (a refusal, an empty field after the first try).
fn error_text<'a>(sentence: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    typo::text(typo::body(), sentence)
        .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) })
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

/// A list that failed: an alert in the error colour, what failed with the core's words below it,
/// and "Retry".
fn failed_row<'a>(what: &'a str, why: &str) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
    let retry = widgets::text_button(None, tr("Retry"), style::button::secondary, Some(Message::Retry));
    setting_row(Some(alert.into()), what, Some(why.to_string()), retry)
}

/// The arrow at the end of a row that leads to another window: 16, `text_3`.
fn arrow<'a>() -> Element<'a, Message> {
    icon::icon(Icon::ArrowUpRight, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) })
        .into()
}

// ----- Pure parts (unit-tested) ------------------------------------------------------------------

/// How a server stands, as a dot and a word: what a person decides first (tools to review), what
/// keeps it from starting (missing, crashed), then whether it is switched on and running.
pub fn status_of(server: &ExtensionInfo) -> (Dot, String) {
    match &server.status {
        Status::Review => (Dot::Attention, tr("Tools to review").to_string()),
        Status::Missing { what } => (Dot::Attention, what.needs()),
        Status::Crashed { .. } => (Dot::Error, tr("Stopped after errors").to_string()),
        _ if !server.enabled => (Dot::Inactive, tr("Off").to_string()),
        Status::Running => (Dot::Ok, tr("Running").to_string()),
        Status::Starting => (Dot::Inactive, tr("Starting…").to_string()),
        // Not running, but on: it starts with the next call.
        Status::Stopped => (Dot::Ok, tr("On").to_string()),
    }
}

/// What the section's row in the sidebar and the section menu marks (task MA.8): [`Dot::Error`]
/// while a server stopped after errors, else [`Dot::Attention`] while one has tools waiting for a
/// person; `None` when no server needs the person.
pub fn alert(data: &Data) -> Option<Dot> {
    let servers = data.extensions.entries();
    if servers.iter().any(|server| matches!(server.status, Status::Crashed { .. })) {
        return Some(Dot::Error);
    }
    servers.iter().any(|server| server.status == Status::Review).then_some(Dot::Attention)
}

/// The catalog entry `entry` when "Install…" is offered for it: it is in the catalog, nothing it
/// needs is missing on this computer, and no installed server has its id.
pub fn installable<'a>(data: &'a Data, entry: &str) -> Option<&'a CatalogEntry> {
    let taken = data.extensions.entries().iter().any(|server| server.id == entry);
    data.catalog.entries().iter().find(|listed| listed.id == entry && listed.missing.is_empty() && !taken)
}

/// The installed server with the id `id`.
pub fn server_of<'a>(data: &'a Data, id: &str) -> Option<&'a ExtensionInfo> {
    data.extensions.entries().iter().find(|server| server.id == id)
}

/// The version "Update" brings a catalog server to: the catalog's, when it differs from the one the
/// server was installed or last updated in. `None` for one's own server, one whose entry left the
/// catalog, and one in the catalog's version.
pub fn update_available(data: &Data, server: &ExtensionInfo) -> Option<String> {
    let Source::Catalog { entry, version } = &server.source else { return None };
    let listed = data.catalog.entries().iter().find(|listed| &listed.id == entry)?;
    (listed.version != *version).then(|| listed.version.clone())
}

/// The installed server of the catalog entry `entry`, if any.
pub fn installed<'a>(data: &'a Data, entry: &str) -> Option<&'a ExtensionInfo> {
    data.extensions
        .entries()
        .iter()
        .find(|server| matches!(&server.source, Source::Catalog { entry: from, .. } if from == entry))
}

/// What runs for a server: the program with its arguments, or the address.
pub fn address(spec: &ServerSpec) -> String {
    match spec {
        ServerSpec::Stdio { command, args, .. } => {
            std::iter::once(command.as_str()).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
        }
        ServerSpec::Http { url, .. } => url.clone(),
    }
}

/// Whether the assistant may use `tool`, after its client's rights as `clients.list_clients` lists
/// them: write access to `tools` and the tool in its selection. The switch is the other half.
/// `false` while the assistant is off (it has no client).
pub fn assistant_may_use(data: &Data, tool: ToolId) -> bool {
    data.clients
        .iter()
        .find(|client| client.kind == ASSISTANT_CLIENT)
        .is_some_and(|client| client.rights.area(Area::Tools).allows(Access::Write) && client.rights.reaches_tool(tool))
}

/// "Letter", "Report", "Note": the template's name, told apart from the paper "US Letter" and a
/// note of the memory.
pub fn template_name(id: PdfTemplateId) -> &'static str {
    match id {
        PdfTemplateId::Letter => trc("template", "Letter"),
        PdfTemplateId::Report => trc("template", "Report"),
        PdfTemplateId::Note => trc("template", "Note"),
    }
}

/// The margins the picker offers: the even ones, and `current` first when it is none of them.
pub fn margin_options(current: [u8; 4]) -> Vec<Margins> {
    let mut options: Vec<Margins> = EVEN_MARGINS.into_iter().map(|margin| Margins([margin; 4])).collect();
    if !options.contains(&Margins(current)) {
        options.insert(0, Margins(current));
    }
    options
}

/// The last part of a path, for the logo row; the whole path when it has none.
fn file_name(path: &str) -> String {
    Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path).to_string()
}

/// A paper in the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaperChoice(pub Paper);

impl PaperChoice {
    const ALL: [PaperChoice; 3] = [PaperChoice(Paper::A4), PaperChoice(Paper::A5), PaperChoice(Paper::Letter)];
}

impl fmt::Display for PaperChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let word = match self.0 {
            Paper::A4 => "A4",
            Paper::A5 => "A5",
            Paper::Letter => tr("US Letter"),
        };
        f.write_str(word)
    }
}

/// Margins in the picker, top, right, bottom, left: "20 mm", or "20 · 25 · 20 · 25 mm" when they
/// differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Margins(pub [u8; 4]);

impl fmt::Display for Margins {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [top, right, bottom, left] = self.0;
        let n = match self.0.iter().all(|margin| *margin == top) {
            true => top.to_string(),
            false => format!("{top} · {right} · {bottom} · {left}"),
        };
        f.write_str(&trf("{n} mm", &[("n", &n)]))
    }
}

/// A font size in the picker: "11 pt".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Points(pub u8);

impl fmt::Display for Points {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&trf("{n} pt", &[("n", &self.0)]))
    }
}

#[cfg(test)]
mod tests {
    use torro_core::settings::ToolSwitch;
    use torro_ipc::api::{Level, Selection};
    use torro_ipc::extensions::{CatalogEntry, CrashReason, Prerequisite};

    use super::super::launch;
    use super::*;

    /// The demo's data with the PDF tool on, a folder and a logo (`--open connections`).
    fn data() -> Data {
        let mut data = launch::demo_data();
        launch::demo_connections(&mut data);
        data
    }

    /// The one request of an action.
    fn one(action: Action) -> Request {
        match action {
            Action::Send(request) => request,
            other => panic!("expected exactly one request, got {other:?}"),
        }
    }

    /// The template a `pdf_template` request carries.
    fn template_of(request: &Request) -> (PdfTemplateId, PdfTemplate) {
        match request {
            Request::SetSetting { key: SettingKey::PdfTemplate, value } => {
                PdfTemplate::from_value(value).expect("a whole template the core takes")
            }
            other => panic!("expected pdf_template, got {other:?}"),
        }
    }

    /// Task MA.7: the switch sends exactly one `set_setting tool_enabled`, both ways, which the core
    /// reads back.
    #[test]
    fn the_switch_sends_exactly_one_set_setting() {
        let data = data();
        let mut state = State::default();
        for on in [true, false] {
            let request = one(update(&mut state, Message::Switch(ToolId::Pdf, on), &data));
            let Request::SetSetting { key: SettingKey::ToolEnabled, value } = &request else {
                panic!("expected tool_enabled, got {request:?}")
            };
            let switch: ToolSwitch = serde_json::from_value(value.clone()).expect("the core reads it");
            assert_eq!(switch, ToolSwitch { tool: ToolId::Pdf, enabled: on });
        }
    }

    /// Every field of a template sends `pdf_template` with the whole template as loaded and that one
    /// field changed – the logo the demo set included.
    #[test]
    fn every_field_sends_the_whole_template() {
        let data = data();
        let id = PdfTemplateId::Report;
        let stored = data.settings.tools.pdf.template(id).clone();
        assert!(stored.logo.is_some(), "the demo's report has a logo, so a lost one would show");
        let with = |change: &dyn Fn(&mut PdfTemplate)| {
            let mut expected = stored.clone();
            change(&mut expected);
            expected
        };
        let cases = [
            (Message::Paper(id, Paper::Letter), with(&|t| t.paper = Paper::Letter)),
            (Message::Margins(id, [10; 4]), with(&|t| t.margins_mm = [10; 4])),
            (Message::FontSize(id, 13), with(&|t| t.font_size_pt = 13)),
            (Message::PageNumbers(id, false), with(&|t| t.page_numbers = false)),
            (Message::RemoveLogo(id), with(&|t| t.logo = None)),
        ];
        let mut state = State::default();
        for (message, expected) in cases {
            let request = one(update(&mut state, message.clone(), &data));
            assert_eq!(template_of(&request), (id, expected), "{message:?}");
        }
    }

    /// Typing sends nothing; Enter sends the whole template with the line, Escape drops it, starting
    /// another line or leaving the templates' page saves the one before.
    #[test]
    fn a_line_goes_out_whole_when_it_is_saved() {
        let data = data();
        let id = PdfTemplateId::Letter;
        let stored = data.settings.tools.pdf.template(id).clone();
        let mut state = State::default();
        assert!(matches!(update(&mut state, Message::Templates, &data), Action::None));
        assert_eq!(state.page, Page::PdfTemplates);
        let typed = update(&mut state, Message::Type(id, Line::Header, "ACME · {date}".into()), &data);
        assert!(matches!(typed, Action::None), "typing sends nothing");
        let request = one(update(&mut state, Message::SaveLine, &data));
        let expected = PdfTemplate { header: "ACME · {date}".into(), ..stored.clone() };
        assert_eq!(template_of(&request), (id, expected));
        assert_eq!(escape(&state), Some(Message::Back), "saved, so Escape leaves the page");

        // Escape drops a line being typed.
        let _ = update(&mut state, Message::Type(id, Line::Footer, "Draft".into()), &data);
        assert_eq!(escape(&state), Some(Message::DiscardLine));
        assert!(matches!(update(&mut state, Message::DiscardLine, &data), Action::None));
        assert!(matches!(update(&mut state, Message::SaveLine, &data), Action::None), "nothing left to save");

        // Another line saves the one before; a line as stored sends nothing.
        let _ = update(&mut state, Message::Type(id, Line::Footer, "Confidential".into()), &data);
        let request = one(update(&mut state, Message::Type(id, Line::Header, stored.header.clone()), &data));
        let expected = PdfTemplate { footer: "Confidential".into(), ..stored.clone() };
        assert_eq!(template_of(&request), (id, expected));
        assert!(matches!(update(&mut state, Message::SaveLine, &data), Action::None), "the header is as stored");

        // Leaving the page saves what is typed.
        let _ = update(&mut state, Message::Type(id, Line::Footer, "Internal".into()), &data);
        let request = one(update(&mut state, Message::Back, &data));
        assert_eq!(template_of(&request).1.footer, "Internal");
        assert_eq!((state.page.clone(), escape(&state)), (Page::List, None));

        // A line never grows beyond what the core takes, and never breaks.
        let long: String = "x".repeat(PdfTemplate::MAX_LINE_CHARS + 20) + "\n";
        let _ = update(&mut state, Message::Type(id, Line::Header, long), &data);
        let (_, template) = template_of(&one(update(&mut state, Message::SaveLine, &data)));
        assert_eq!(template.header.chars().count(), PdfTemplate::MAX_LINE_CHARS);
    }

    /// Folder and logo: the choosers are the shell's; what comes back carries the path, the logo
    /// inside the whole template; cancelled, nothing; a path that is no text, a sentence above the
    /// page. "Use desktop" removes the folder.
    #[test]
    fn the_choosers_are_the_shell_s_and_send_the_path() {
        let data = data();
        let mut state = State::default();
        let id = PdfTemplateId::Note;
        for chooser in [Chooser::Folder, Chooser::Logo(id)] {
            assert!(matches!(update(&mut state, Message::Choose(chooser), &data), Action::Pick(c) if c == chooser));
            assert!(matches!(update(&mut state, Message::Picked(chooser, None), &data), Action::None), "cancelled");
        }
        assert_eq!(
            one(update(&mut state, Message::UseDesktop, &data)),
            Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::Value::Null }
        );
        let folder = PathBuf::from("/home/anna/PDFs");
        assert_eq!(
            one(update(&mut state, Message::Picked(Chooser::Folder, Some(folder)), &data)),
            Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::json!("/home/anna/PDFs") }
        );
        let logo = PathBuf::from("/home/anna/logo.png");
        let request = one(update(&mut state, Message::Picked(Chooser::Logo(id), Some(logo.clone())), &data));
        let expected = PdfTemplate { logo: Some("/home/anna/logo.png".into()), ..data.settings.tools.pdf.note.clone() };
        assert_eq!(template_of(&request), (id, expected));
        assert!(LOGO_EXTENSIONS.iter().all(|ext| logo_request(&data, id, &logo.with_extension(ext)).is_some()));
        assert!(state.problem.is_none());
    }

    /// A path the system hands over that is not plain text cannot be stored: nothing is sent, the
    /// page says so until the next chooser opens.
    #[cfg(unix)]
    #[test]
    fn a_path_that_is_no_text_is_refused_with_a_sentence() {
        use std::os::unix::ffi::OsStrExt;
        let data = data();
        let mut state = State::default();
        let odd = PathBuf::from(std::ffi::OsStr::from_bytes(b"/home/anna/\xff"));
        assert!(matches!(update(&mut state, Message::Picked(Chooser::Folder, Some(odd)), &data), Action::None));
        assert_eq!(state.problem.as_deref(), Some(tr("This path cannot be used.")));
        let _ = update(&mut state, Message::Choose(Chooser::Folder), &data);
        assert!(state.problem.is_none());
    }

    /// Nothing here reaches the clients: every request is a setting or brings the assistant's window
    /// to the front ("may not use it"); the rest is the shell's chooser, a page or a reload.
    #[test]
    fn no_message_sends_a_clients_request() {
        let data = data();
        let mut state = State::default();
        let messages = [
            Message::Switch(ToolId::Pdf, true),
            Message::Access,
            Message::Choose(Chooser::Folder),
            Message::Picked(Chooser::Folder, Some(PathBuf::from("/tmp/pdfs"))),
            Message::UseDesktop,
            Message::Templates,
            Message::Paper(PdfTemplateId::Letter, Paper::A5),
            Message::Margins(PdfTemplateId::Letter, [30; 4]),
            Message::FontSize(PdfTemplateId::Letter, 9),
            Message::Type(PdfTemplateId::Letter, Line::Header, "x".into()),
            Message::SaveLine,
            Message::DiscardLine,
            Message::PageNumbers(PdfTemplateId::Letter, true),
            Message::Choose(Chooser::Logo(PdfTemplateId::Letter)),
            Message::RemoveLogo(PdfTemplateId::Letter),
            Message::Back,
            Message::Retry,
        ];
        for message in messages {
            match update(&mut state, message.clone(), &data) {
                Action::Send(Request::OpenApp { app, target: None }) => {
                    assert_eq!(app, torro_core::AppId::ASSISTANT.binary, "{message:?}");
                }
                Action::Send(request) => assert_eq!(request.operation().area(), Area::Settings, "{message:?}"),
                Action::Pick(_) | Action::Reload | Action::None => {}
                other => panic!("{message:?} did {other:?}"),
            }
        }
    }

    /// The assistant may use a tool with write access to `tools` and the tool chosen; reading or a
    /// selection without it is not enough, and an assistant that is off may use nothing.
    #[test]
    fn whether_the_assistant_may_use_it_follows_its_rights() {
        let mut data = data();
        assert!(assistant_may_use(&data, ToolId::Pdf), "the demo's assistant may make PDFs");
        fn rights(data: &mut Data) -> &mut torro_ipc::api::Rights {
            &mut data.clients.iter_mut().find(|client| client.kind == ASSISTANT_CLIENT).expect("the assistant").rights
        }
        rights(&mut data).tools = Selection::Only(Vec::new());
        assert!(!assistant_may_use(&data, ToolId::Pdf), "not in the selection");
        rights(&mut data).tools = Selection::All;
        rights(&mut data).set_level(Area::Tools, Level::Read, Vec::new);
        assert!(!assistant_may_use(&data, ToolId::Pdf), "reading only");
        rights(&mut data).set_level(Area::Tools, Level::Write, || vec!["pdf".into()]);
        assert!(assistant_may_use(&data, ToolId::Pdf));
        data.clients.retain(|client| client.kind != ASSISTANT_CLIENT);
        assert!(!assistant_may_use(&data, ToolId::Pdf), "the assistant is off");
    }

    /// A server's status in words, with the dot's colour as the second sign: what a person must do
    /// first, then whether it is on.
    #[test]
    fn a_server_says_how_it_stands() {
        let data = launch::demo_data();
        let mut server = data.extensions.entries().first().cloned().expect("the demo has servers");
        let cases = [
            (Status::Running, true, Dot::Ok, tr("Running").to_string()),
            (Status::Stopped, true, Dot::Ok, tr("On").to_string()),
            (Status::Starting, true, Dot::Inactive, tr("Starting…").to_string()),
            (Status::Running, false, Dot::Inactive, tr("Off").to_string()),
            (Status::Review, false, Dot::Attention, tr("Tools to review").to_string()),
            (Status::Missing { what: Prerequisite::Npx }, true, Dot::Attention, Prerequisite::Npx.needs()),
            (
                Status::Crashed { reason: CrashReason::Exited },
                false,
                Dot::Error,
                tr("Stopped after errors").to_string(),
            ),
        ];
        for (status, enabled, dot, word) in cases {
            (server.status, server.enabled) = (status.clone(), enabled);
            assert_eq!(status_of(&server), (dot, word), "{status:?}, on: {enabled}");
        }
        let program = ServerSpec::Stdio {
            command: "npx".into(),
            args: vec!["-y".into(), "notes".into()],
            env: Default::default(),
        };
        assert_eq!(address(&program), "npx -y notes");
        let web = ServerSpec::Http { url: "https://mcp.example.org/".into(), headers: Default::default() };
        assert_eq!(address(&web), "https://mcp.example.org/");
    }

    /// The demo's catalog knows which entries are installed; its servers of one's own are the others.
    #[test]
    fn the_catalog_knows_what_is_installed() {
        let data = launch::demo_data();
        assert!(installed(&data, "time").is_some_and(|server| server.status == Status::Running));
        assert!(installed(&data, "sequential-thinking").is_none());
        let own = data.extensions.entries().iter().filter(|server| server.source == Source::Custom).count();
        assert!(own > 0 && count(&data) > own, "both kinds, so both groups show rows");
        let missing = data.catalog.entries().iter().find(|entry| entry.id == "sequential-thinking");
        assert!(missing.is_some_and(|entry| entry.missing == [Prerequisite::Npx]), "the demo has no Node");
    }

    /// The pickers' words and the margins set elsewhere.
    #[test]
    fn the_pickers_say_what_they_hold() {
        assert_eq!(margin_options([20; 4]).len(), EVEN_MARGINS.len());
        let odd = margin_options([20, 25, 20, 25]);
        assert_eq!(odd[0], Margins([20, 25, 20, 25]), "set through the API, listed first");
        assert_eq!(Margins([20; 4]).to_string(), trf("{n} mm", &[("n", &20)]));
        assert_eq!(Margins([20, 25, 20, 25]).to_string(), trf("{n} mm", &[("n", &"20 · 25 · 20 · 25")]));
        assert_eq!(Points(11).to_string(), trf("{n} pt", &[("n", &11)]));
        assert_eq!(PaperChoice(Paper::Letter).to_string(), tr("US Letter"));
        assert_eq!(template_name(PdfTemplateId::Letter), trc("template", "Letter"));
        assert_eq!(file_name("/home/anna/logo.png"), "logo.png");
    }

    /// Every state builds in every size: both pages, a line typed, servers listed, none, and both
    /// lists failed; the header names the templates' page.
    #[test]
    fn the_section_builds_in_every_state() {
        let data = data();
        let mut empty = data.clone();
        empty.extensions = Listing::Entries(Vec::new());
        empty.tools.clear();
        let mut failed = data.clone();
        failed.extensions = Listing::Failed("unsupported".into());
        failed.catalog = Listing::Failed("unsupported".into());
        assert!(failed.extensions.entries().is_empty() && count(&failed) == 0);
        let mut typing = State::default();
        let _ = update(&mut typing, Message::Type(PdfTemplateId::Report, Line::Footer, "Draft".into()), &data);
        let templates = State { page: Page::PdfTemplates, ..State::default() };
        let list = State::default();
        assert!(header(&list, &data, SizeClass::Wide).sub_page.is_none());
        assert!(
            header(&templates, &data, SizeClass::Wide)
                .sub_page
                .is_some_and(|(name, back)| { name == tr("PDF templates") && back == Message::Back })
        );
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            for state in [&list, &typing, &templates] {
                for data in [&data, &empty, &failed] {
                    let _ = view(state, data, class);
                }
            }
        }
        assert_eq!(Listing::from_result(Err::<Vec<CatalogEntry>, _>("no".into())), Listing::Failed("no".into()));
    }

    // ----- Task MA.8: catalog, installing, a server's page ------------------------------------------

    /// The request and what it is for of an action of this section.
    fn asked(action: Action) -> (Asked, Request) {
        match action {
            Action::AskConnections(asked, request) => (asked, request),
            other => panic!("expected exactly one request of the section, got {other:?}"),
        }
    }

    /// The text an action puts on the clipboard.
    fn copied(action: Action) -> Option<String> {
        match action {
            Action::Copy(text) => Some(text),
            _ => None,
        }
    }

    /// The demo's data without its GitHub server, so its catalog entry can be installed.
    fn uninstalled() -> Data {
        let mut data = launch::demo_data();
        launch::demo_uninstalled(&mut data);
        data
    }

    const TOKEN_NAME: &str = "GITHUB_PERSONAL_ACCESS_TOKEN";

    /// A server as the core installed it: off, its tools waiting.
    fn reported(data: &Data, entry: &str) -> ExtensionInfo {
        match launch::demo_answer(&Request::GetExtension { id: entry.into() }, data) {
            Some(Response::Extension { extension }) => extension,
            other => panic!("expected the server, got {other:?}"),
        }
    }

    /// The install dialog of GitHub at the tools it reported, as the demo installs it.
    fn at_its_tools(data: &Data) -> State {
        let mut state = State::default();
        let _ = update(&mut state, Message::Install("github".into()), data);
        let _ = update(&mut state, Message::TypeSecret(TOKEN_NAME.into(), Secret::new("t")), data);
        let (what, _) = asked(update(&mut state, Message::SubmitInstall, data));
        let _ = answered(&mut state, what, &Ok(Response::Extension { extension: reported(data, "github") }));
        assert!(state.install.as_ref().is_some_and(|install| install.installed() == Some("github")));
        state
    }

    /// "Install…" only for an entry with nothing missing that is not installed: the demo has no Node,
    /// so Sequential Thinking has none, and Time is installed.
    #[test]
    fn install_is_offered_only_without_anything_missing() {
        let data = uninstalled();
        assert!(installable(&data, "github").is_some());
        assert!(installable(&data, "sequential-thinking").is_none(), "needs Node.js, which the demo lacks");
        assert!(installable(&data, "time").is_none(), "installed already");
        assert!(installable(&data, "nothing").is_none());
        let mut state = State::default();
        for entry in ["sequential-thinking", "time"] {
            assert!(matches!(update(&mut state, Message::Install(entry.into()), &data), Action::None));
            assert!(state.install.is_none() && overlay(&state, &data).is_none(), "{entry}: no dialog");
        }
        let entry = data.catalog.entries().iter().find(|entry| entry.id == "sequential-thinking").expect("listed");
        assert_eq!(
            missing_sentence(entry),
            trf("{needs}. Install it to use this server.", &[("needs", &tr("Needs Node.js (npx)"))])
        );
        let _ = update(&mut state, Message::Install("github".into()), &data);
        assert!(overlay(&state, &data).is_some());
    }

    /// "Install" sends exactly one `install_extension` with the typed secrets as `Secret`s – only
    /// after every secret of the entry is typed, else the dialog says so after the first press. The
    /// answer shows the reported tools, and "Allow tools" sends `confirm_tools` with exactly the
    /// fingerprint of that list; allowed, the dialog closes.
    #[test]
    fn installing_sends_install_then_allow_with_the_fingerprint_shown() {
        let data = uninstalled();
        let mut state = State::default();
        let _ = update(&mut state, Message::Install("github".into()), &data);
        assert!(matches!(update(&mut state, Message::SubmitInstall, &data), Action::None), "no token yet");
        assert!(state.install.as_ref().is_some_and(|install| install.tried && !install.busy));
        let _ = update(&mut state, Message::TypeSecret(TOKEN_NAME.into(), Secret::new("github_pat_123")), &data);
        let (what, request) = asked(update(&mut state, Message::SubmitInstall, &data));
        let secrets = BTreeMap::from([(TOKEN_NAME.to_string(), Secret::new("github_pat_123"))]);
        let from = InstallFrom::Catalog { entry: "github".into(), secrets };
        assert_eq!((what.clone(), request), (Asked::Install("github".into()), Request::InstallExtension { from }));
        assert!(matches!(update(&mut state, Message::SubmitInstall, &data), Action::None), "on its way: once");

        let mut server = reported(&data, "github");
        server.fingerprint = Some("list-the-person-saw".into());
        let next = answered(&mut state, what, &Ok(Response::Extension { extension: server.clone() }));
        assert!(matches!(next, Action::None));
        assert_eq!(state.install.as_ref().and_then(Install::installed), Some("github"));
        let allow = Message::AllowTools("github".into(), "list-the-person-saw".into());
        let (what, request) = asked(update(&mut state, allow, &data));
        let confirm = Request::ConfirmExtensionTools { id: "github".into(), fingerprint: "list-the-person-saw".into() };
        assert_eq!((what.clone(), request), (Asked::Allow("github".into()), confirm));
        let _ = answered(&mut state, what, &Ok(Response::Extension { extension: server }));
        assert!(state.install.is_none(), "allowed: the dialog has done its job");
    }

    /// The button of the dialog carries the fingerprint of the list it shows.
    #[test]
    fn the_allow_button_carries_the_fingerprint_of_the_list_shown() {
        let data = uninstalled();
        let server = reported(&data, "github");
        let press = |server: &ExtensionInfo| {
            server.fingerprint.clone().map(|fingerprint| Message::AllowTools(server.id.clone(), fingerprint))
        };
        assert_eq!(press(&server), Some(Message::AllowTools("github".into(), "demo-github".into())));
        // The dialog and the page build with a server whose list was never read: no button to press.
        let mut unread = server.clone();
        unread.fingerprint = None;
        let _ = allow_button(&unread, false, false, true);
        assert_eq!(press(&unread), None);
    }

    /// A refused install says what that means in the dialog by the core's code; tools that changed
    /// under the dialog are read again and shown for another look.
    #[test]
    fn refusals_show_in_the_dialog_with_a_clear_sentence() {
        let data = uninstalled();
        let refused = |code: &str| Ok(Response::Error { code: code.into(), message: format!("core says {code}") });
        for (code, what) in [
            ("unsupported", tr("Something it needs is missing on this computer.")),
            ("conflict", tr("It is installed already.")),
            ("error", tr("It could not be installed.")),
        ] {
            let mut state = State::default();
            let _ = update(&mut state, Message::Install("github".into()), &data);
            let _ = update(&mut state, Message::TypeSecret(TOKEN_NAME.into(), Secret::new("t")), &data);
            let (asked_for, _) = asked(update(&mut state, Message::SubmitInstall, &data));
            let _ = answered(&mut state, asked_for, &refused(code));
            let install = state.install.as_ref().expect("still open");
            assert_eq!(install.problem, Some(Problem { what: Some(what), why: format!("core says {code}") }));
            assert!(!install.busy && install.installed().is_none(), "{code}: the form again");
            let _ = view(&state, &data, SizeClass::Wide);
            assert!(overlay(&state, &data).is_some());
        }
        let no_answer = Err("The core did not answer".to_string());
        let mut state = State::default();
        let _ = update(&mut state, Message::Install("github".into()), &data);
        let _ = answered(&mut state, Asked::Install("github".into()), &no_answer);
        assert!(state.install.as_ref().is_some_and(|install| install.problem.is_none()), "not asked: not taken");

        // The list changed: the server is read again, its new list shown.
        let server = reported(&data, "github");
        let mut state = at_its_tools(&data);
        let fingerprint = server.fingerprint.clone().expect("read");
        let _ = update(&mut state, Message::AllowTools("github".into(), fingerprint), &data);
        let (what, request) = asked(answered(&mut state, Asked::Allow("github".into()), &refused("conflict")));
        assert_eq!(
            (what.clone(), request),
            (Asked::Refresh("github".into()), Request::GetExtension { id: "github".into() })
        );
        let mut changed = server;
        changed.fingerprint = Some("the new list".into());
        let _ = answered(&mut state, what, &Ok(Response::Extension { extension: changed }));
        let Some(Install { step: Step::Tools(shown), busy: false, .. }) = &state.install else {
            panic!("the tools again")
        };
        assert_eq!(shown.fingerprint.as_deref(), Some("the new list"));
    }

    /// Every button of a server's page sends exactly its request: opening reads the output, the
    /// switch, "Allow tools", "Replace…" with "Save", "Refresh", "Update" and "Remove…" with its
    /// question; "Copy" copies what was read; leaving the page forgets it.
    #[test]
    fn every_button_of_a_server_s_page_sends_exactly_its_request() {
        let data = launch::demo_data();
        let mut state = State::default();
        let id = || "github".to_string();
        let (what, request) = asked(update(&mut state, Message::Open(id()), &data));
        assert_eq!((what.clone(), request), (Asked::Logs(id()), Request::ReadExtensionLogs { id: id() }));
        assert_eq!(state.page, Page::Server(id()));
        let lines = vec!["one".to_string(), "two".to_string()];
        let _ = answered(&mut state, what, &Ok(Response::Logs { lines }));
        assert_eq!(copied(update(&mut state, Message::CopyOutput, &data)), Some("one\ntwo".to_string()));
        let typed = text_editor::Action::Edit(text_editor::Edit::Insert('x'));
        let _ = update(&mut state, Message::Output(typed), &data);
        assert_eq!(copied(update(&mut state, Message::CopyOutput, &data)), Some("one\ntwo".to_string()), "shows only");

        let cases = [
            (
                Message::Enable(id(), false),
                Asked::Switch(id()),
                Request::SetExtensionEnabled { id: id(), enabled: false },
            ),
            (
                Message::AllowTools(id(), "demo-github".into()),
                Asked::Allow(id()),
                Request::ConfirmExtensionTools { id: id(), fingerprint: "demo-github".into() },
            ),
            (Message::ReadOutput, Asked::Logs(id()), Request::ReadExtensionLogs { id: id() }),
            (Message::Update(id()), Asked::Update(id()), Request::UpdateExtension { id: id(), server: None }),
        ];
        for (message, what, request) in cases {
            assert_eq!(asked(update(&mut state, message.clone(), &data)), (what.clone(), request), "{message:?}");
            let _ = answered(&mut state, what, &Ok(Response::Ok));
        }

        let _ = update(&mut state, Message::Replace(TOKEN_NAME.into()), &data);
        assert!(overlay(&state, &data).is_some());
        assert!(matches!(update(&mut state, Message::SubmitReplacement, &data), Action::None), "empty: only says so");
        let _ = update(&mut state, Message::TypeReplacement(Secret::new("github_pat_new")), &data);
        let (what, request) = asked(update(&mut state, Message::SubmitReplacement, &data));
        let secret =
            Request::SetExtensionSecret { id: id(), name: TOKEN_NAME.into(), value: Secret::new("github_pat_new") };
        assert_eq!((what.clone(), request), (Asked::Secret(id()), secret));
        let _ = answered(&mut state, what, &Ok(Response::Ok));
        assert!(state.server.replacing.is_none(), "stored: the dialog closes");

        assert!(matches!(update(&mut state, Message::AskRemove, &data), Action::None), "asks first");
        assert_eq!(escape(&state), Some(Message::CancelRemove));
        let (what, request) = asked(update(&mut state, Message::Remove, &data));
        assert_eq!((what.clone(), request), (Asked::Remove(id()), Request::RemoveExtension { id: id() }));
        let _ = answered(&mut state, what, &Ok(Response::Ok));
        assert_eq!(state.page, Page::List, "removed: back to the list");

        let _ = update(&mut state, Message::Open(id()), &data);
        assert!(header(&state, &data, SizeClass::Wide).sub_page.is_some_and(|(name, _)| name == "GitHub"));
        assert_eq!(escape(&state), Some(Message::Back));
        let _ = update(&mut state, Message::Back, &data);
        assert!(matches!(state.server.output, Output::Reading) && state.page == Page::List);
        let link = update(&mut state, Message::Link("https://example.org".into()), &data);
        assert!(matches!(link, Action::OpenUrl(url) if url == "https://example.org"));
    }

    /// A refusal on a server's page shows above it; an answer for a page left meanwhile changes nothing.
    #[test]
    fn a_refusal_on_a_server_s_page_shows_above_it() {
        let data = launch::demo_data();
        let mut state = State::default();
        let _ = update(&mut state, Message::Open("notes".into()), &data);
        let _ = update(&mut state, Message::Enable("notes".into(), true), &data);
        let refused = Ok(Response::Error { code: "conflict".into(), message: "Allow its tools first".into() });
        let _ = answered(&mut state, Asked::Switch("notes".into()), &refused);
        assert_eq!(state.server.problem.as_deref(), Some("Allow its tools first"));
        assert!(state.server.waiting.is_none());
        let _ = answered(&mut state, Asked::Remove("time".into()), &Ok(Response::Ok));
        assert_eq!(state.page, Page::Server("notes".into()), "another server's answer");
        let _ = view(&state, &data, SizeClass::Narrow);
    }

    /// What is typed into a secret never shows in the state's `Debug`: in the install dialog and in
    /// the dialog that replaces one.
    #[test]
    fn secrets_never_show_in_the_debug_of_the_state() {
        let data = uninstalled();
        let mut state = State::default();
        let _ = update(&mut state, Message::Install("github".into()), &data);
        let _ = update(&mut state, Message::TypeSecret(TOKEN_NAME.into(), Secret::new("ghp_install_secret")), &data);
        let message = Message::TypeReplacement(Secret::new("ghp_replace_secret"));
        assert!(!format!("{message:?}").contains("ghp_replace_secret"));
        let _ = update(&mut state, Message::Open("time".into()), &data);
        let _ = update(&mut state, Message::Replace("TOKEN".into()), &data);
        let _ = update(&mut state, message, &data);
        let (_, request) = asked(update(&mut state, Message::SubmitReplacement, &data));
        let debug = format!("{state:?} {request:?}");
        assert!(debug.contains(TOKEN_NAME) && debug.contains("TOKEN"), "the names show: {debug}");
        assert!(!debug.contains("ghp_install_secret") && !debug.contains("ghp_replace_secret"), "{debug}");
    }

    /// The sidebar marks the section while a server stopped after errors (first) or waits for its
    /// tools; the version "Update" brings is the catalog's when it differs.
    #[test]
    fn the_sidebar_marks_servers_that_need_the_person() {
        let mut data = launch::demo_data();
        assert_eq!(alert(&data), Some(Dot::Error), "GitHub stopped after errors");
        launch::demo_uninstalled(&mut data);
        assert_eq!(alert(&data), Some(Dot::Attention), "Notes waits for its tools");
        data.extensions = Listing::Entries(Vec::new());
        assert_eq!(alert(&data), None);

        let mut data = launch::demo_data();
        let time = server_of(&data, "time").cloned().expect("Time");
        assert_eq!(update_available(&data, &time), None, "in the catalog's version");
        let mut old = time;
        old.source = Source::Catalog { entry: "time".into(), version: "2025.1.1".into() };
        let catalog = data.catalog.entries().iter().find(|entry| entry.id == "time").map(|entry| entry.version.clone());
        assert_eq!(update_available(&data, &old), catalog);
        assert_eq!(update_available(&data, server_of(&data, "notes").expect("Notes")), None, "one's own");
        launch::demo_review(&mut data);
        let time = server_of(&data, "time").expect("Time");
        let changed = time.tools.iter().find(|tool| tool.name == "convert_time").expect("changed");
        assert_eq!(tool_state(time, changed), (Dot::Attention, tr("Changed since you allowed it").to_string()));
        let fresh = reported(&uninstalled(), "github");
        assert_eq!(tool_state(&fresh, &fresh.tools[0]), (Dot::Attention, tr("Not allowed yet").to_string()));
    }

    /// Every new state builds in every size: the catalog with "Install…", both steps of the dialog
    /// busy and not, each server's page with its output read, failed or empty, and its dialogs.
    #[test]
    fn the_catalog_the_dialog_and_the_pages_build_in_every_size() {
        let data = uninstalled();
        let full = launch::demo_data();
        let mut form = State::default();
        let _ = update(&mut form, Message::Install("github".into()), &data);
        let _ = update(&mut form, Message::SubmitInstall, &data);
        let mut busy = State::default();
        let _ = update(&mut busy, Message::Install("github".into()), &data);
        let _ = update(&mut busy, Message::TypeSecret(TOKEN_NAME.into(), Secret::new("t")), &data);
        let _ = update(&mut busy, Message::SubmitInstall, &data);
        let tools = at_its_tools(&data);
        let mut pages = Vec::new();
        for id in ["time", "github", "notes", "gone"] {
            let mut page = State::default();
            let _ = update(&mut page, Message::Open(id.into()), &full);
            pages.push(page);
            let mut read = State::default();
            let (what, _) = asked(update(&mut read, Message::Open(id.into()), &full));
            let _ = answered(&mut read, what, &Ok(Response::Logs { lines: vec!["started".into()] }));
            let _ = update(&mut read, Message::Replace(TOKEN_NAME.into()), &full);
            pages.push(read);
            let mut failed = State::default();
            let (what, _) = asked(update(&mut failed, Message::Open(id.into()), &full));
            let _ = answered(&mut failed, what, &Err("no answer".into()));
            let _ = update(&mut failed, Message::AskRemove, &full);
            pages.push(failed);
        }
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            for state in [&form, &busy, &tools] {
                let _ = view(state, &data, class);
                assert!(overlay(state, &data).is_some());
            }
            for state in &pages {
                let _ = view(state, &full, class);
                let _ = overlay(state, &full);
            }
            let _ = view(&State::default(), &full, class);
        }
    }
}
