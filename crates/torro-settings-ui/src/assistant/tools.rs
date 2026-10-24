//! Assistant › Tools (task WZ.6, issue #225, concept `docs/assistent.md` "Eigene Werkzeuge"): the
//! assistant's own tools, each with its switch, whether the assistant may use it, and its settings.
//!
//! ```text
//! PDF       Make PDFs (switch) · whether the assistant may use it · Folder (Choose…, Use desktop)
//! Letter    Paper · Margins · Font size · Header · Footer · Page numbers · Logo (Choose…, Remove)
//! Report    the same
//! Note      the same
//! ```
//!
//! **Switch and rights are two things** (`docs/assistent.md`): the switch (`settings.set_setting
//! tool_enabled`) says whether the tool exists on this computer, for every client; what the
//! assistant may use is its rights (`tools` with the tools chosen), which Access in this window
//! edits (task S.34). So the row "may use it" only reads the assistant's client
//! (`clients.list_clients`, as General does) and opens Access ([`crate::Action::Open`], at the
//! assistant's own rights); nothing here writes a client.
//!
//! **Only a person switches a tool on and sets a path** (point 9 of "Was nie nach außen geht" in
//! `docs/api.md`): this window runs with the app key, so it may. Folder and logo come from the
//! system's chooser, which only the app can open ([`super::AppAction::PickFolder`],
//! [`super::AppAction::PickLogo`]); the app then sends [`folder_request`] or [`logo_request`].
//!
//! Every change of a template field sends `settings.set_setting pdf_template` with the **whole**
//! template as loaded, the one field changed ([`template_request`]): the key replaces a template
//! whole, so a field left out would be reset. Header and footer are typed: they go out with Enter or
//! "Save", or when another line is started, never on every key ([`State`]).

use std::fmt;
use std::path::Path;

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Fill};

use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::{Paper, PdfTemplate, PdfTemplateId, ToolId};
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, Status};
use torro_gui::{style, typo};
use torro_ipc::api::Area;
use torro_ipc::tools::{ToolInfo, access_to_use};
use torro_ipc::{Request, SettingKey};

use super::{Action, AppAction, access};
use crate::Data;
use crate::parts::{chevron, field, picker, pressable, setting_row, switch};

/// The file endings the logo chooser offers, as `PdfTemplate::check` takes them.
pub const LOGO_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// The even margins the picker offers, in millimetres; a margin set otherwise through the API is
/// listed among them ([`margin_options`]).
const EVEN_MARGINS: [u8; 8] = [5, 10, 15, 20, 25, 30, 40, 50];

/// The section's own state: the header or footer line being typed, until it is saved.
#[derive(Debug, Default)]
pub struct State {
    draft: Option<Draft>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// The switch of a tool: on or off for every client (`tool_enabled`).
    Switch(ToolId, bool),
    /// "Allow it under Access": Access in this window, at the assistant's own rights (the
    /// sections open it there, [`super::update`]).
    Access,
    /// "Choose…" at the folder: the system's folder chooser, in the app.
    ChooseFolder,
    /// "Use desktop": no folder of its own any more (`pdf_folder` `null`).
    UseDesktop,
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
    /// "Choose…" at a template's logo: the system's file chooser, in the app.
    ChooseLogo(PdfTemplateId),
    /// "Remove" at a template's logo.
    RemoveLogo(PdfTemplateId),
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Switch(tool, on) => Action::send(switch_request(tool, on)),
        Message::Access => Action::Open(access::ID),
        Message::ChooseFolder => Action::App(AppAction::PickFolder),
        Message::UseDesktop => {
            Action::send(Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::Value::Null })
        }
        Message::Paper(id, paper) => changed(data, id, |template| template.paper = paper),
        Message::Margins(id, margins) => changed(data, id, |template| template.margins_mm = margins),
        Message::FontSize(id, points) => changed(data, id, |template| template.font_size_pt = points),
        Message::PageNumbers(id, on) => changed(data, id, |template| template.page_numbers = on),
        Message::RemoveLogo(id) => changed(data, id, |template| template.logo = None),
        Message::ChooseLogo(id) => Action::App(AppAction::PickLogo(id)),
        Message::Type(id, line, text) => {
            // Starting another line saves the one before, so nothing typed is dropped silently.
            let before = state.draft.take().filter(|draft| draft.template != id || draft.line != line);
            let text = text.chars().filter(|c| !c.is_control()).take(PdfTemplate::MAX_LINE_CHARS).collect();
            state.draft = Some(Draft { template: id, line, text });
            before.and_then(|draft| line_request(data, &draft)).map_or(Action::None, Action::send)
        }
        Message::SaveLine => {
            state.draft.take().and_then(|draft| line_request(data, &draft)).map_or(Action::None, Action::send)
        }
        Message::DiscardLine => {
            state.draft = None;
            Action::None
        }
    }
}

/// What Escape means here: drop the line being typed. `None` while nothing is typed.
pub fn escape(state: &State) -> Option<Message> {
    state.draft.as_ref().map(|_| Message::DiscardLine)
}

// ----- Requests (also the app's, after a chooser) -------------------------------------------

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

/// `settings.set_setting pdf_folder` with the folder the person chose in the system's chooser, for
/// the app after [`AppAction::PickFolder`]. `None` for a path that is not plain text (it could not
/// be stored as the person sees it).
pub fn folder_request(folder: &Path) -> Option<Request> {
    let path = folder.to_str()?;
    Some(Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::Value::String(path.to_string()) })
}

/// `settings.set_setting pdf_template` with the template `id` as `data` holds it now and `logo` as
/// its logo, for the app after [`AppAction::PickLogo`]. `None` for a path that is not plain text.
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
    Action::send(template_request(id, &template))
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

// ----- Views --------------------------------------------------------------------------------

/// A group per tool, and for the PDF tool its three templates. In a narrow window the buttons are
/// icons with a tooltip, so the labels keep their room (design skill, section 6).
pub fn view<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    if !super::is_on(data) {
        return super::off_group(tr("Tools"));
    }
    if data.tools.is_empty() {
        let row = setting_row(None, tr("No tools"), None, Space::new());
        return widgets::group(tr("Tools"), vec![row], None);
    }
    let words = matches!(class, SizeClass::Wide | SizeClass::Medium);
    let mut page = column![].spacing(layout::section_gap());
    for info in &data.tools {
        page = page.push(tool_group(info, data, words));
        if info.id == ToolId::Pdf {
            let last = PdfTemplateId::ALL.len() - 1;
            for (at, id) in PdfTemplateId::ALL.into_iter().enumerate() {
                page = page.push(template_group(state, data, id, words, at == last));
            }
        }
    }
    page.into()
}

/// One tool: its switch with what it does, whether the assistant may use it, and for the PDF tool
/// the folder its files go to.
fn tool_group<'a>(info: &'a ToolInfo, data: &'a Data, words: bool) -> Element<'a, Message> {
    let id = info.id;
    let on = wrapping_row(
        switch_label(id),
        info.description.as_str(),
        switch(info.enabled, move |on| Message::Switch(id, on)),
    );
    let mut rows = vec![on, access_row(data, id)];
    if id == ToolId::Pdf {
        rows.push(folder_row(data, words));
    }
    let footer = tr(
        "A tool works only while it is on here and allowed under Access. Only you can switch it on and choose the folder and the logos.",
    );
    widgets::group(info.name.as_str(), rows, Some(footer.to_string()))
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
        ToolId::Files => tr("Organize files"),
    }
}

/// Whether the assistant may use the tool: a green dot, or an orange one (the tool is on, but the
/// assistant may not use it: something to do) and a row that opens Access, where its rights are
/// chosen – a chevron, not the arrow of another window: Access is in this one.
fn access_row<'a>(data: &Data, id: ToolId) -> Element<'a, Message> {
    if assistant_may_use(data, id) {
        return setting_row(Some(widgets::status_dot(Status::Ok)), tr("The assistant may use it"), None, Space::new());
    }
    let detail = tr("Allow it under Access");
    let row = setting_row(
        Some(widgets::status_dot(Status::Attention)),
        tr("The assistant may not use it"),
        Some(detail.to_string()),
        chevron(),
    );
    pressable(row, Some(Message::Access))
}

/// "Folder": where new PDFs go, the desktop without one; "Choose…" and, with one, "Use desktop".
fn folder_row<'a>(data: &'a Data, words: bool) -> Element<'a, Message> {
    let folder = data.settings.tools.pdf.folder.as_deref();
    let detail = folder.map_or_else(|| tr("Desktop").to_string(), str::to_string);
    let mut buttons = row![choose_button(words, Message::ChooseFolder)].spacing(space::s());
    if folder.is_some() {
        buttons = buttons.push(match words {
            true => widgets::text_button(None, tr("Use desktop"), style::button::ghost, Some(Message::UseDesktop)),
            // Not the ✕: nothing is deleted, the folder goes back to the desktop.
            false => widgets::icon_button(Icon::History, tr("Use desktop"), Some(Message::UseDesktop)),
        });
    }
    setting_row(None, tr("Folder"), Some(detail), buttons.align_y(Alignment::Center))
}

/// One template: paper, margins, font size, header, footer, page numbers and logo. The `last` one
/// says below it what header and footer fill in.
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
    let mut buttons = row![choose_button(words, Message::ChooseLogo(id))].spacing(space::s());
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
fn choose_button<'a>(words: bool, message: Message) -> Element<'a, Message> {
    match words {
        true => widgets::text_button(None, tr("Choose…"), style::button::secondary, Some(message)),
        false => widgets::icon_button(Icon::Folder, tr("Choose…"), Some(message)),
    }
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// Whether the assistant may use `tool`, after its client's rights as `clients.list_clients` lists
/// them: the access the tool needs in `tools` ([`access_to_use`]: write for `pdf`, read for `files`)
/// and the tool in its selection – what `tools.list_tools` calls `allowed` for a client key (this
/// window asks with the app key, which may use every tool). The switch is the other half. `false`
/// while the assistant is off.
pub fn assistant_may_use(data: &Data, tool: ToolId) -> bool {
    super::client(data).is_some_and(|client| {
        client.rights.area(Area::Tools).allows(access_to_use(tool)) && client.rights.reaches_tool(tool)
    })
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
    use std::path::PathBuf;

    use torro_core::settings::ToolSwitch;
    use torro_ipc::api::{Level, Selection};

    use super::super::tests::{data, turned_off};
    use super::*;
    use crate::Action as Act;

    /// The one request of an action.
    fn one(action: Action) -> Request {
        match action {
            Act::Send(mut requests) if requests.len() == 1 => requests.remove(0),
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

    /// The switch sends exactly one `set_setting tool_enabled`, both ways, which the core reads back.
    #[tokio::test]
    async fn the_switch_sends_exactly_one_set_setting() {
        let data = data().await;
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
    #[tokio::test]
    async fn every_field_sends_the_whole_template() {
        let data = data().await;
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

    /// Typing sends nothing; Enter sends the whole template with the line, Escape drops it, and
    /// starting another line saves the one before.
    #[tokio::test]
    async fn a_line_goes_out_whole_when_it_is_saved() {
        let data = data().await;
        let id = PdfTemplateId::Letter;
        let stored = data.settings.tools.pdf.template(id).clone();
        let mut state = State::default();
        let typed = update(&mut state, Message::Type(id, Line::Header, "ACME · {date}".into()), &data);
        assert_eq!(typed, Act::None, "typing sends nothing");
        let request = one(update(&mut state, Message::SaveLine, &data));
        let expected = PdfTemplate { header: "ACME · {date}".into(), ..stored.clone() };
        assert_eq!(template_of(&request), (id, expected));
        assert!(escape(&state).is_none(), "saved, nothing left to drop");

        // Escape drops a line being typed.
        let _ = update(&mut state, Message::Type(id, Line::Footer, "Draft".into()), &data);
        assert_eq!(escape(&state), Some(Message::DiscardLine));
        assert_eq!(update(&mut state, Message::DiscardLine, &data), Act::None);
        assert_eq!(update(&mut state, Message::SaveLine, &data), Act::None, "nothing left to save");

        // Another line saves the one before; a line as stored sends nothing.
        let _ = update(&mut state, Message::Type(id, Line::Footer, "Confidential".into()), &data);
        let request = one(update(&mut state, Message::Type(id, Line::Header, stored.header.clone()), &data));
        let expected = PdfTemplate { footer: "Confidential".into(), ..stored.clone() };
        assert_eq!(template_of(&request), (id, expected));
        assert_eq!(update(&mut state, Message::SaveLine, &data), Act::None, "the header is as stored");

        // A line never grows beyond what the core takes, and never breaks.
        let long: String = "x".repeat(PdfTemplate::MAX_LINE_CHARS + 20) + "\n";
        let _ = update(&mut state, Message::Type(id, Line::Header, long), &data);
        let (_, template) = template_of(&one(update(&mut state, Message::SaveLine, &data)));
        assert_eq!(template.header.chars().count(), PdfTemplate::MAX_LINE_CHARS);
    }

    /// Folder and logo: the choosers are the app's; what it sends afterwards carries the path, the
    /// logo inside the whole template. "Use desktop" removes the folder.
    #[tokio::test]
    async fn the_choosers_are_the_app_s_and_send_the_path() {
        let data = data().await;
        let mut state = State::default();
        assert_eq!(update(&mut state, Message::ChooseFolder, &data), Act::App(AppAction::PickFolder));
        let id = PdfTemplateId::Note;
        assert_eq!(update(&mut state, Message::ChooseLogo(id), &data), Act::App(AppAction::PickLogo(id)));
        assert_eq!(
            one(update(&mut state, Message::UseDesktop, &data)),
            Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::Value::Null }
        );

        let folder = PathBuf::from("/home/anna/PDFs");
        assert_eq!(
            folder_request(&folder),
            Some(Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::json!("/home/anna/PDFs") })
        );
        let logo = PathBuf::from("/home/anna/logo.png");
        let request = logo_request(&data, id, &logo).expect("a plain path");
        let expected = PdfTemplate { logo: Some("/home/anna/logo.png".into()), ..data.settings.tools.pdf.note.clone() };
        assert_eq!(template_of(&request), (id, expected));
        assert!(LOGO_EXTENSIONS.iter().all(|ext| logo_request(&data, id, &logo.with_extension(ext)).is_some()));
    }

    /// Task WZ.6: nothing here reaches the clients – every action is a setting, Access (task S.34)
    /// or the app's chooser. The row "may use it" only reads the client and points away.
    #[tokio::test]
    async fn no_message_sends_a_clients_request() {
        let data = data().await;
        let mut state = State::default();
        let messages = [
            Message::Switch(ToolId::Pdf, true),
            Message::Access,
            Message::ChooseFolder,
            Message::UseDesktop,
            Message::Paper(PdfTemplateId::Letter, Paper::A5),
            Message::Margins(PdfTemplateId::Letter, [30; 4]),
            Message::FontSize(PdfTemplateId::Letter, 9),
            Message::Type(PdfTemplateId::Letter, Line::Header, "x".into()),
            Message::SaveLine,
            Message::DiscardLine,
            Message::PageNumbers(PdfTemplateId::Letter, true),
            Message::ChooseLogo(PdfTemplateId::Letter),
            Message::RemoveLogo(PdfTemplateId::Letter),
        ];
        for message in messages {
            match update(&mut state, message.clone(), &data) {
                Act::Send(requests) => {
                    for request in requests {
                        assert_ne!(request.operation().area(), Area::Clients, "{message:?}");
                        assert_eq!(request.operation().area(), Area::Settings, "{message:?}");
                    }
                }
                Act::Open(section) => assert_eq!(section, access::ID),
                Act::App(_) | Act::None => {}
                other => panic!("{message:?} did {other:?}"),
            }
        }
    }

    /// The assistant may use a tool with write access to `tools` and the tool chosen; reading or a
    /// selection without it is not enough, and an assistant that is off may use nothing.
    #[tokio::test]
    async fn whether_the_assistant_may_use_it_follows_its_rights() {
        let mut data = data().await;
        assert!(!assistant_may_use(&data, ToolId::Pdf), "the area starts closed");
        fn rights(data: &mut Data) -> &mut torro_ipc::api::Rights {
            &mut data.clients[0].rights
        }
        rights(&mut data).set_level(Area::Tools, Level::Write, || vec!["pdf".into()]);
        assert!(assistant_may_use(&data, ToolId::Pdf));
        rights(&mut data).tools = Selection::Only(Vec::new());
        assert!(!assistant_may_use(&data, ToolId::Pdf), "not in the selection");
        rights(&mut data).tools = Selection::All;
        rights(&mut data).set_level(Area::Tools, Level::Read, Vec::new);
        assert!(!assistant_may_use(&data, ToolId::Pdf), "reading only");
        assert!(assistant_may_use(&data, ToolId::Files), "reading is enough to organize files (issue #304)");
        rights(&mut data).tools = Selection::Only(vec!["pdf".into()]);
        assert!(!assistant_may_use(&data, ToolId::Files), "files not in the selection");
        assert!(!assistant_may_use(&turned_off(&data), ToolId::Pdf));
        let mut state = State::default();
        assert_eq!(update(&mut state, Message::Access, &data), Act::Open("access"));
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

    /// The demo (`--demo`): the PDF tool on, the assistant without access, a folder and a logo, so
    /// every row shows; built in every size, on, off and without tools, and while a line is typed.
    #[tokio::test]
    async fn the_section_builds_in_every_state() {
        let data = data().await;
        let pdf = data.tools.iter().find(|tool| tool.id == ToolId::Pdf).expect("the demo lists the PDF tool");
        assert!(pdf.enabled && data.settings.tools.is_on(ToolId::Pdf), "switched on in the demo");
        assert!(data.settings.tools.pdf.folder.is_some(), "a folder of its own");
        assert!(!assistant_may_use(&data, ToolId::Pdf), "so the row leads to Access");
        let mut typing = State::default();
        let _ = update(&mut typing, Message::Type(PdfTemplateId::Report, Line::Footer, "Draft".into()), &data);
        let no_tools = Data { tools: Vec::new(), ..data.clone() };
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            for state in [&State::default(), &typing] {
                let _ = view(state, &data, class);
            }
            let _ = view(&typing, &no_tools, class);
            let _ = view(&typing, &turned_off(&data), class);
        }
    }
}
