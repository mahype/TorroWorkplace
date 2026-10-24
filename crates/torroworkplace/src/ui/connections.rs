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
//!
//! Where the next tasks build on this:
//!
//! - MA.8 (catalog, installing, a server's page): a server's page becomes a [`Page`] of its own
//!   (`Page::Server(id)`, its name in [`header`]); the rows of [`catalog_group`] and [`own_group`]
//!   become [`pressable`] with a [`chevron`], a catalog row gets "Install…"; the install dialog is
//!   state here and a new `pub fn overlay`, which `ui/mod.rs` lays over the window like the other
//!   sections'. [`status_of`] is the status in words for the server's page and its sidebar mark.
//! - MA.9 (a server of one's own): "Add server…" in the actions of [`header`] on the list, its form as
//!   state here and part of the same `overlay`.

use std::fmt;
use std::path::{Path, PathBuf};

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, svg};
use iced::{Alignment, Element, Fill, Theme};

use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::{Paper, PdfTemplate, PdfTemplateId, ToolId};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Status as Dot};
use torro_gui::{style, typo};
use torro_ipc::api::{ASSISTANT_CLIENT, Access, Area};
use torro_ipc::extensions::{ExtensionInfo, ServerSpec, Source, Status};
use torro_ipc::tools::ToolInfo;
use torro_ipc::{Request, SettingKey};

use super::{Action, ClientTarget, Data, Header, chevron, field, notice, picker, pressable, setting_row, switch};

/// The file endings the logo chooser offers, as `PdfTemplate::check` takes them.
pub const LOGO_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// The even margins the picker offers, in millimetres; a margin set otherwise through the API is
/// listed among them ([`margin_options`]).
const EVEN_MARGINS: [u8; 8] = [5, 10, 15, 20, 25, 30, 40, 50];

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
}

/// The section's state: the page shown, the header or footer line being typed, and why the last
/// chosen path could not be used.
#[derive(Debug, Default)]
pub struct State {
    pub page: Page,
    draft: Option<Draft>,
    /// Shown above the page until the next chooser opens.
    problem: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    }
}

/// What Escape means here: drop the line being typed, else leave the templates' page. `None` on the
/// list while nothing is typed.
pub fn escape(state: &State) -> Option<Message> {
    match (&state.draft, &state.page) {
        (Some(_), _) => Some(Message::DiscardLine),
        (None, Page::PdfTemplates) => Some(Message::Back),
        (None, Page::List) => None,
    }
}

/// The header bar: on the templates' page their name with a back button; the list has no actions
/// yet ("Add server…" comes with task MA.9).
pub fn header<'a>(state: &'a State, _data: &'a Data, _class: SizeClass) -> Header<'a, Message> {
    match state.page {
        Page::PdfTemplates => {
            Header { sub_page: Some((tr("PDF templates").to_string(), Message::Back)), actions: vec![] }
        }
        Page::List => Header::default(),
    }
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
    let body: Element<'a, Message> = match state.page {
        Page::List => column![tools_group(data, words), catalog_group(data), own_group(data)]
            .spacing(layout::section_gap())
            .into(),
        Page::PdfTemplates => templates_page(state, data, words),
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

/// "Catalog": a row per entry of the core's catalog with the status of the installed server, then the
/// installed catalog servers whose entry the catalog no longer has. A failed list is a sentence with
/// "Retry".
fn catalog_group(data: &Data) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = match &data.catalog {
        Listing::Failed(why) => vec![failed_row(tr("The catalog could not be listed"), why)],
        Listing::Entries(entries) => entries
            .iter()
            .map(|entry| server_row(&entry.name, entry.description.clone(), installed(data, &entry.id)))
            .collect(),
    };
    let known = |entry: &str| data.catalog.entries().iter().any(|listed| listed.id == entry);
    rows.extend(
        data.extensions
            .entries()
            .iter()
            .filter(|server| matches!(&server.source, Source::Catalog { entry, .. } if !known(entry)))
            .map(|server| server_row(&server.name, tr("No longer in the catalog").to_string(), Some(server))),
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
            .map(|server| server_row(&server.name, address(&server.server), Some(server)))
            .collect(),
    };
    if rows.is_empty() {
        rows.push(setting_row(None, tr("No servers of your own yet"), None, Space::new()));
    }
    let footer = tr("Programs or addresses that speak MCP, added by you. They run as you on this computer.");
    widgets::group(tr("Your own servers"), rows, Some(footer.to_string()))
}

/// A server: its name, `detail` below it, and the status of the installed one at the end – a dot
/// and a word, so it never rests on colour alone.
fn server_row<'a>(name: &'a str, detail: String, server: Option<&ExtensionInfo>) -> Element<'a, Message> {
    let status: Element<'a, Message> = match server {
        Some(server) => {
            let (dot, word) = status_of(server);
            row![widgets::status_dot(dot), typo::text(typo::meta(), word).style(theme::text_2).wrapping(Wrapping::None)]
                .spacing(space::s())
                .align_y(Alignment::Center)
                .into()
        }
        None => Space::new().into(),
    };
    setting_row(None, name, Some(detail), status)
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
}
