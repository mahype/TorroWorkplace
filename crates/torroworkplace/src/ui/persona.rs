//! Assistant › Profile and Assistant › Memory (task C.10 in `docs/tasks/assistent.md`, concept
//! `docs/assistent.md`, "Persönlichkeit (Stufe 1)" and decision 1): who the assistant is and what it
//! keeps because the person asked it to. Two pages below the Assistant page, opened from its rows
//! "Profile" and "Memory" ([`profile_rows`]).
//!
//! - **Profile:** name, short description, picture (the first letter of the name or one of the
//!   suite's symbols, [`torro_ipc::assistant::AVATARS`]) and the personality, typed into a draft and
//!   saved with "Save" (`assistant.update_profile`, only the fields that changed). Three templates
//!   ("Calm and brief", "Friendly", "Formal") fill the personality as a starting point;
//!   "Discard changes" brings back what is stored. A blank name and a personality over 4,000
//!   characters are named after the first "Save", not before.
//! - **Memory:** the notes (`assistant.list_memories`), each with "Edit" (`assistant.remember` with
//!   `replaces`) and "Forget" (`assistant.forget`, after asking); "Add note" in the header. A note
//!   kept from models outside this computer says so ("This computer only").
//!
//! The page reads and writes nothing itself: the profile and the notes come with [`Data`]
//! (`assistant.get_profile`, `assistant.list_memories`), every change is a request. The draft follows
//! a reload while nothing is typed, like a mail account's sender name.

use chrono::Utc;
use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, text_editor};
use iced::{Alignment, Background, Element, Fill, Theme, border};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_short_date, to_local};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Mark};
use torro_gui::{style, typo};
use torro_ipc::assistant::{
    AVATARS, DEFAULT_NAME, LETTER_AVATAR, MAX_NOTE_CHARS, MAX_PERSONALITY_CHARS, Note, Profile,
};
use torro_ipc::{Request, Response};

use super::{Action, Data, Header, chevron, confirm, pressable, row_frame, setting_row, wizard};

/// The id of the field a note is typed into: it takes the focus when "Add note" or "Edit" opens it.
pub const NOTE_FIELD: &str = "assistant-note";

/// The page below Assistant that is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub {
    Profile,
    Memory,
}

/// A starting point for the personality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Calm,
    Friendly,
    Formal,
}

impl Template {
    pub const ALL: [Template; 3] = [Template::Calm, Template::Friendly, Template::Formal];

    pub fn label(self) -> &'static str {
        match self {
            Template::Calm => tr("Calm and brief"),
            Template::Friendly => tr("Friendly"),
            Template::Formal => tr("Formal"),
        }
    }

    /// The personality it fills in, in the language of the suite: the person reads and changes it.
    pub fn text(self) -> &'static str {
        match self {
            Template::Calm => tr(
                "You are calm and brief. Answer in a few clear sentences, without small talk. Say what you did and what comes next. Ask only when you really need to know something.",
            ),
            Template::Friendly => tr(
                "You are warm and friendly, like a helpful colleague. Speak casually and keep answers short. When it helps, suggest a next step.",
            ),
            Template::Formal => tr(
                "You are polite and formal. Write complete sentences without slang or emoji. Be precise, and give dates and times in full.",
            ),
        }
    }
}

/// Page state.
#[derive(Debug, Default)]
pub struct State {
    /// The page below Assistant that is open; `None` on the Assistant page itself.
    pub sub: Option<Sub>,
    /// What is typed into the profile; made when its page opens.
    draft: Option<Draft>,
    /// A note being written (`id: None`) or changed.
    edit: Option<NoteEdit>,
    /// "Forget this note?" for the note with this id.
    forget: Option<String>,
    /// "Discard changes?" before the back button leaves a profile with unsaved changes.
    leaving: bool,
}

/// The profile as it is being typed, and as it was stored when the draft was made.
#[derive(Debug)]
struct Draft {
    name: String,
    description: String,
    avatar: String,
    personality: text_editor::Content,
    stored: Profile,
    /// "Save" was pressed once: problems show from now on.
    tried: bool,
}

impl Draft {
    fn new(profile: &Profile) -> Draft {
        Draft {
            name: profile.name.clone(),
            description: profile.description.clone(),
            avatar: profile.avatar.clone(),
            personality: text_editor::Content::with_text(&profile.personality),
            stored: profile.clone(),
            tried: false,
        }
    }

    /// The personality as it would be sent: line breaks as `\n`, no white space at the ends.
    fn personality_text(&self) -> String {
        self.personality.text().replace("\r\n", "\n").trim().to_string()
    }

    /// What changed against `stored`, as `assistant.update_profile`; `None` when nothing did.
    fn changes(&self) -> Option<Request> {
        let changed = |typed: &str, stored: &str| (typed.trim() != stored.trim()).then(|| typed.trim().to_string());
        let request = Request::UpdateProfile {
            name: changed(&self.name, &self.stored.name),
            description: changed(&self.description, &self.stored.description),
            avatar: (self.avatar != self.stored.avatar).then(|| self.avatar.clone()),
            personality: changed(&self.personality_text(), &self.stored.personality),
        };
        match &request {
            Request::UpdateProfile { name: None, description: None, avatar: None, personality: None } => None,
            _ => Some(request),
        }
    }

    /// What keeps "Save" from sending, for people; `None` when it can.
    fn problem(&self) -> Option<String> {
        let count = self.personality_text().chars().count();
        if self.name.trim().is_empty() {
            Some(tr("Give the assistant a name.").to_string())
        } else if count > MAX_PERSONALITY_CHARS {
            Some(trf(
                "The personality is too long: {count} of {max} characters.",
                &[("count", &count), ("max", &MAX_PERSONALITY_CHARS)],
            ))
        } else {
            None
        }
    }
}

/// A note as it is being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NoteEdit {
    /// The note it changes; `None` for a new one.
    id: Option<String>,
    text: String,
    /// "Save" was pressed once: a problem shows from now on.
    tried: bool,
}

impl NoteEdit {
    fn problem(&self) -> Option<String> {
        let count = self.text.trim().chars().count();
        if count == 0 {
            Some(tr("Write the note first.").to_string())
        } else if count > MAX_NOTE_CHARS {
            Some(trf("A note holds at most {max} characters.", &[("max", &MAX_NOTE_CHARS)]))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The rows on the Assistant page.
    OpenProfile,
    OpenMemory,
    /// The back button in the header: asks first while the profile has unsaved changes.
    Back,
    /// "Discard" in "Discard changes?": back without saving.
    Leave,
    Name(String),
    Description(String),
    Avatar(String),
    Personality(text_editor::Action),
    Template(Template),
    /// "Save" under the profile.
    Save,
    /// "Discard changes".
    Discard,
    /// "Add note" in the header.
    AddNote,
    /// "Edit" at the note with this id.
    EditNote(String),
    NoteInput(String),
    /// "Save" at the note being typed, Enter in its field.
    SaveNote,
    /// "Cancel" at the note being typed, Escape.
    CancelNote,
    /// "Forget" at the note with this id: asks first.
    AskForget(String),
    Forget,
    CloseDialog,
}

/// The profile the core holds, or the defaults before it answered.
fn profile_of(data: &Data) -> Profile {
    data.profile.clone().unwrap_or_else(|| Profile {
        name: DEFAULT_NAME.to_string(),
        description: tr("Your assistant for this computer").to_string(),
        avatar: LETTER_AVATAR.to_string(),
        personality: String::new(),
    })
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::OpenProfile => {
            state.sub = Some(Sub::Profile);
            state.draft = Some(Draft::new(&profile_of(data)));
            Action::None
        }
        Message::OpenMemory => {
            state.sub = Some(Sub::Memory);
            state.edit = None;
            Action::None
        }
        Message::Back if state.draft.as_ref().is_some_and(|draft| draft.changes().is_some()) => {
            state.leaving = true;
            Action::None
        }
        Message::Back | Message::Leave => {
            *state = State::default();
            Action::None
        }
        Message::Name(name) => edit_draft(state, |draft| draft.name = name),
        Message::Description(description) => edit_draft(state, |draft| draft.description = description),
        Message::Avatar(avatar) => edit_draft(state, |draft| draft.avatar = avatar),
        Message::Personality(action) => edit_draft(state, |draft| draft.personality.perform(action)),
        Message::Template(template) => {
            edit_draft(state, |draft| draft.personality = text_editor::Content::with_text(template.text()))
        }
        Message::Save => save(state),
        Message::Discard => {
            if let Some(draft) = &mut state.draft {
                *draft = Draft::new(&draft.stored.clone());
            }
            Action::None
        }
        Message::AddNote => {
            state.edit = Some(NoteEdit { id: None, text: String::new(), tried: false });
            Action::Focus(NOTE_FIELD)
        }
        Message::EditNote(id) => {
            let text = data.memories.iter().find(|note| note.id == id).map(|note| note.text.clone());
            state.edit = text.map(|text| NoteEdit { id: Some(id), text, tried: false });
            if state.edit.is_some() { Action::Focus(NOTE_FIELD) } else { Action::None }
        }
        Message::NoteInput(text) => {
            if let Some(edit) = &mut state.edit {
                edit.text = text;
            }
            Action::None
        }
        Message::SaveNote => save_note(state),
        Message::CancelNote => {
            state.edit = None;
            Action::None
        }
        Message::AskForget(id) => {
            state.forget = data.memories.iter().any(|note| note.id == id).then_some(id);
            Action::None
        }
        Message::Forget => match state.forget.take() {
            Some(id) => Action::Send(Request::Forget { id }),
            None => Action::None,
        },
        Message::CloseDialog => {
            state.forget = None;
            state.leaving = false;
            Action::None
        }
    }
}

/// Changes the draft, when there is one.
fn edit_draft(state: &mut State, change: impl FnOnce(&mut Draft)) -> Action {
    if let Some(draft) = &mut state.draft {
        change(draft);
    }
    Action::None
}

/// "Save": what changed, unless a problem keeps it back (shown from now on).
fn save(state: &mut State) -> Action {
    let Some(draft) = &mut state.draft else { return Action::None };
    draft.tried = true;
    if draft.problem().is_some() {
        return Action::None;
    }
    draft.changes().map_or(Action::None, Action::Send)
}

/// "Save" at a note: `assistant.remember`, with `replaces` for a changed note.
fn save_note(state: &mut State) -> Action {
    let Some(edit) = &mut state.edit else { return Action::None };
    edit.tried = true;
    if edit.problem().is_some() {
        return Action::None;
    }
    Action::Send(Request::Remember { text: edit.text.trim().to_string(), replaces: edit.id.clone(), local_only: false })
}

/// The answer to a request the shell sent: a stored profile takes the draft's place, a stored note
/// ends its editing. Errors leave both as typed (the shell shows the error).
pub fn answered(state: &mut State, result: &Result<Response, String>) {
    match result {
        Ok(Response::Profile { profile }) if state.draft.is_some() => state.draft = Some(Draft::new(profile)),
        Ok(Response::Memory { .. }) => state.edit = None,
        _ => {}
    }
}

/// The shell loaded [`Data`] anew: the draft follows the stored profile while nothing is typed; a
/// note being changed that is gone ends its editing.
pub fn loaded(state: &mut State, data: &Data) {
    if let (Some(draft), Some(profile)) = (&mut state.draft, &data.profile)
        && draft.changes().is_none()
        && draft.stored != *profile
    {
        *draft = Draft::new(profile);
    }
    if let Some(NoteEdit { id: Some(id), .. }) = &state.edit
        && !data.memories.iter().any(|note| &note.id == id)
    {
        state.edit = None;
    }
}

/// What Escape means here: close the question, then stop typing a note.
pub fn escape(state: &State) -> Option<Message> {
    if state.forget.is_some() || state.leaving {
        Some(Message::CloseDialog)
    } else if state.edit.is_some() {
        Some(Message::CancelNote)
    } else {
        None
    }
}

/// "Forget this note?" while it is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if state.leaving {
        return Some(confirm(
            tr("Discard changes?"),
            tr("What you changed in the profile is not saved.").to_string(),
            tr("Discard"),
            Message::Leave,
            Message::CloseDialog,
        ));
    }
    let id = state.forget.as_ref()?;
    let note = data.memories.iter().find(|note| &note.id == id)?;
    Some(confirm(
        tr("Forget this note?"),
        trf("The assistant will no longer know: “{note}” This cannot be undone.", &[("note", &note.text)]),
        tr("Forget"),
        Message::Forget,
        Message::CloseDialog,
    ))
}

/// The header of a page below Assistant: its name after the section's and the back button; on
/// Memory "Add note", the window's one primary button (only its icon in narrow windows).
pub fn header<'a>(state: &'a State, class: SizeClass) -> Option<Header<'a, Message>> {
    match state.sub? {
        Sub::Profile => {
            Some(Header { sub_page: Some((tr("Profile").to_string(), Message::Back)), actions: Vec::new() })
        }
        Sub::Memory => {
            let label = tr("Add note");
            let adding = matches!(state.edit, Some(NoteEdit { id: None, .. }));
            let on_press = (!adding).then_some(Message::AddNote);
            let button = match class {
                SizeClass::Wide | SizeClass::Medium => {
                    widgets::text_button(Some(Icon::Plus), label, style::button::primary, on_press)
                }
                SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
                    widgets::compact_button(Icon::Plus, label, style::button::primary, on_press)
                }
            };
            Some(Header { sub_page: Some((tr("Memory").to_string(), Message::Back)), actions: vec![button] })
        }
    }
}

// ----- Views --------------------------------------------------------------------------------

/// The open page below Assistant; `None` on the Assistant page itself.
pub fn view<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    match state.sub? {
        Sub::Profile => state.draft.as_ref().map(profile_page),
        Sub::Memory => Some(memory_page(state, data)),
    }
}

/// The rows on the Assistant page that open the two pages: the profile with its picture, name and
/// description, the memory with how many notes it holds.
pub fn profile_rows<'a>(data: &'a Data) -> Element<'a, Message> {
    let profile = profile_of(data);
    let face = avatar(&profile.name, &profile.avatar, size::control_sm());
    let description = Some(profile.description.clone()).filter(|text| !text.is_empty());
    let profile_row = setting_row(Some(face), profile.name, description, chevron());
    let count = data.memories.len();
    let detail = match count {
        0 => tr("Nothing remembered yet").to_string(),
        1 => tr("1 note").to_string(),
        n => trf("{count} notes", &[("count", &n)]),
    };
    // In a box as wide as the picture, so both titles stand on one axis.
    let mark = container(
        icon::icon(Icon::AlignLeft, icon::SIZE)
            .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) }),
    )
    .center(size::control_sm());
    let memory_row = setting_row(Some(mark.into()), tr("Memory"), Some(detail), chevron());
    let footer = tr("Who the assistant is, how it speaks, and what it keeps because you asked it to.");
    widgets::group(
        tr("Profile"),
        vec![pressable(profile_row, Some(Message::OpenProfile)), pressable(memory_row, Some(Message::OpenMemory))],
        Some(footer.to_string()),
    )
}

/// The personality's field shows this many lines before it scrolls.
const PERSONALITY_LINES: f32 = 10.0;

/// The room of the personality's field: [`PERSONALITY_LINES`] of text and the padding of a field.
fn personality_height() -> f32 {
    PERSONALITY_LINES * typo::line_height(typo::body()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// Inside a group, around what is no setting row (chips, a text field): 12 above and below, 16 at
/// the sides.
fn inset<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content).width(Fill).padding([space::m(), space::l()]).into()
}

/// Name, description, picture and personality, then "Save" and "Discard changes".
fn profile_page(draft: &Draft) -> Element<'_, Message> {
    let save_on = draft.changes().is_some();
    let name = wizard::field(DEFAULT_NAME, &draft.name).on_input(Message::Name).on_submit(Message::Save);
    let description = wizard::field(tr("Your assistant for this computer"), &draft.description)
        .on_input(Message::Description)
        .on_submit(Message::Save);
    let preview = avatar(&draft.name, &draft.avatar, size::control_lg());
    let choices = AVATARS
        .iter()
        .map(|id| {
            let (label, mark) = avatar_choice(id);
            let on = draft.avatar == *id;
            widgets::choice_chip(label, mark, on, Some(Message::Avatar((*id).to_string())))
        })
        .collect();
    let picture = row![preview, widgets::chip_row(choices)].spacing(space::l()).align_y(Alignment::Center);
    let typed = draft.personality_text();
    let templates = Template::ALL
        .into_iter()
        .map(|template| {
            widgets::choice_chip(template.label(), None, typed == template.text(), Some(Message::Template(template)))
        })
        .collect();
    let editor = text_editor(&draft.personality)
        .on_action(Message::Personality)
        .placeholder(tr("How should the assistant speak and behave?"))
        .font(typo::body().font)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .padding(style::INPUT_PADDING)
        .wrapping(Wrapping::WordOrGlyph)
        .height(personality_height())
        .style(style::text_editor);
    let count = typed.chars().count();
    let counter = typo::text(
        typo::meta().tabular(),
        trf("{count} of {max} characters", &[("count", &count), ("max", &MAX_PERSONALITY_CHARS)]),
    )
    .style(theme::text_3);
    let personality = column![
        typo::text(typo::caption(), tr("Start from")).style(theme::text_3),
        widgets::chip_row(templates),
        editor,
        row![blank::horizontal(), counter],
    ]
    .spacing(space::s());
    let save_style: widgets::ButtonStyle = if save_on { style::button::primary } else { style::button::secondary };
    let mut buttons = row![blank::horizontal()].spacing(space::s()).align_y(Alignment::Center);
    if save_on {
        buttons = buttons.push(widgets::text_button(
            None,
            tr("Discard changes"),
            style::button::ghost,
            Some(Message::Discard),
        ));
    }
    buttons = buttons.push(widgets::text_button(None, tr("Save"), save_style, save_on.then_some(Message::Save)));
    let mut page = column![
        widgets::group(tr("Name"), vec![row_frame(name, false)], None),
        widgets::group(
            tr("Description"),
            vec![row_frame(description, false)],
            Some(tr("Shown under the name in the chat.").to_string()),
        ),
        widgets::group(tr("Picture"), vec![inset(picture)], None),
        widgets::group(
            tr("Personality"),
            vec![inset(personality)],
            Some(
                tr("How the assistant speaks and behaves. It never changes what the assistant may use, and the rules of Torro Workplace always come first.")
                    .to_string(),
            ),
        ),
    ]
    .spacing(layout::section_gap());
    // A problem shows after the first "Save" only, next to the button that found it.
    let problem = draft.problem().filter(|_| draft.tried);
    let mut bottom = column![buttons].spacing(space::s());
    if let Some(problem) = problem {
        bottom = bottom.push(row![blank::horizontal(), problem_text(problem)]);
    }
    page = page.push(bottom);
    page.into()
}

/// The notes with "Edit" and "Forget", a note being typed first when it is new.
fn memory_page<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let today = to_local(Utc::now()).date();
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    if let Some(edit @ NoteEdit { id: None, .. }) = &state.edit {
        rows.push(note_editor(edit));
    }
    // Newest first: a note just added stands where it was typed.
    for note in data.memories.iter().rev() {
        match &state.edit {
            Some(edit) if edit.id.as_deref() == Some(note.id.as_str()) => rows.push(note_editor(edit)),
            _ => rows.push(note_row(note, today)),
        }
    }
    if rows.is_empty() {
        let hint = tr("Say “Remember …” in a chat, or add a note here.");
        rows.push(setting_row(None, tr("Nothing remembered yet"), Some(hint.to_string()), Space::new()));
    }
    let footer = tr(
        "The assistant reads these notes in every chat and keeps only what you ask it to. Notes marked “This computer only” stay with models on this computer.",
    );
    widgets::group(tr("Notes"), rows, Some(footer.to_string()))
}

/// One note: its text (wrapping), when it was written and whether it stays on this computer, and
/// "Edit" and "Forget".
fn note_row<'a>(note: &'a Note, today: chrono::NaiveDate) -> Element<'a, Message> {
    let mut meta = format_short_date(to_local(note.created).date(), today);
    if note.local_only {
        meta = format!("{meta} · {}", tr("This computer only"));
    }
    let text = column![
        typo::text(typo::body(), note.text.as_str()).wrapping(Wrapping::WordOrGlyph),
        typo::text(typo::meta(), meta).style(theme::text_3),
    ]
    .spacing(space::xs())
    .width(Fill);
    let actions = row![
        widgets::icon_button(Icon::SquarePen, tr("Edit"), Some(Message::EditNote(note.id.clone()))),
        widgets::icon_button(Icon::Trash2, tr("Forget"), Some(Message::AskForget(note.id.clone()))),
    ]
    .spacing(space::xs());
    // The buttons belong to the first line of a long note.
    inset(row![text, actions].spacing(space::l()).align_y(Alignment::Start))
}

/// A note being typed: the field, "Cancel" and "Save"; Enter saves, Escape cancels. A problem shows
/// after the first "Save".
fn note_editor(edit: &NoteEdit) -> Element<'_, Message> {
    let field = wizard::field(tr("For example: Anna works in sales."), &edit.text)
        .id(NOTE_FIELD)
        .on_input(Message::NoteInput)
        .on_submit(Message::SaveNote);
    let buttons = row![
        blank::horizontal(),
        widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CancelNote)),
        widgets::text_button(None, tr("Save"), style::button::secondary, Some(Message::SaveNote)),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let mut content = column![field, buttons].spacing(space::s());
    if let Some(problem) = edit.problem().filter(|_| edit.tried) {
        content = content.push(problem_text(problem));
    }
    inset(content)
}

/// A problem of a form: an alert icon in the error colour and the words in `text`; the colour is
/// only in the icon.
fn problem_text<'a>(problem: String) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE_SM)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).error_text) });
    row![alert, typo::text(typo::meta(), problem)].spacing(space::xs()).align_y(Alignment::Center).into()
}

/// The word and the mark of a picture in the picker.
fn avatar_choice(id: &str) -> (&'static str, Option<Mark>) {
    match avatar_icon(id) {
        Some(icon) => (avatar_label(id), Some(Mark::Icon(icon))),
        None => (tr("Letter"), None),
    }
}

/// What a picture is called.
fn avatar_label(id: &str) -> &'static str {
    match id {
        "sparkles" => tr("Sparkles"),
        "sun" => tr("Sun"),
        "moon" => tr("Moon"),
        "cloud" => tr("Cloud"),
        "radar" => tr("Radar"),
        "user" => tr("Person"),
        _ => tr("Letter"),
    }
}

/// The symbol of a picture; `None` for the letter and for a picture this window does not know.
pub fn avatar_icon(id: &str) -> Option<Icon> {
    match id {
        "sparkles" => Some(Icon::Sparkles),
        "sun" => Some(Icon::Sun),
        "moon" => Some(Icon::Moon),
        "cloud" => Some(Icon::Cloud),
        "radar" => Some(Icon::Radar),
        "user" => Some(Icon::User),
        _ => None,
    }
}

/// The assistant's picture, round, `edge` across: its symbol, else the first letter of `name`
/// (`?` without one), in `text` on `selected` – neutral, the red is the app's.
pub fn avatar<'a, M: 'a>(name: &str, avatar: &str, edge: f32) -> Element<'a, M> {
    // A symbol keeps a ring of room in a small circle.
    let symbol_edge = if edge < size::control_sm() { icon::SIZE_SM } else { icon::SIZE };
    let face: Element<'a, M> = match avatar_icon(avatar) {
        Some(symbol) => icon::icon(symbol, symbol_edge)
            .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text) })
            .into(),
        None => {
            let letter = name.trim().chars().next().map_or_else(|| "?".to_string(), |c| c.to_uppercase().to_string());
            let style = if edge >= size::control_lg() { typo::title() } else { typo::caption() };
            typo::text(style, letter).style(theme::text).wrapping(Wrapping::None).into()
        }
    };
    container(face)
        .center(edge)
        .style(move |t: &Theme| container::Style {
            background: Some(Background::Color(palette(t).selected)),
            border: border::rounded(edge / 2.0),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn stored() -> Profile {
        Profile {
            name: "Torro".into(),
            description: "Your assistant for this computer".into(),
            avatar: LETTER_AVATAR.into(),
            personality: "Be brief.".into(),
        }
    }

    fn note(id: &str, text: &str, local_only: bool) -> Note {
        let created = Utc.with_ymd_and_hms(2026, 10, 14, 9, 0, 0).single().expect("a time");
        Note { id: id.into(), text: text.into(), created, local_only }
    }

    fn data() -> Data {
        Data {
            profile: Some(stored()),
            memories: vec![note("1a2b3c4d", "Anna works in sales.", false), note("5e6f7a8b", "ACME owes 400 €.", true)],
            ..Data::default()
        }
    }

    fn sent(action: Action) -> Request {
        match action {
            Action::Send(request) => request,
            other => panic!("expected a request, got {other:?}"),
        }
    }

    #[test]
    fn saving_the_profile_sends_only_what_changed() {
        let data = data();
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenProfile, &data);
        assert_eq!(state.sub, Some(Sub::Profile));
        assert!(matches!(update(&mut state, Message::Save, &data), Action::None), "nothing changed");

        let _ = update(&mut state, Message::Name("Jarvis".into()), &data);
        let _ = update(&mut state, Message::Avatar("sun".into()), &data);
        let _ = update(&mut state, Message::Template(Template::Formal), &data);
        let request = sent(update(&mut state, Message::Save, &data));
        assert_eq!(
            request,
            Request::UpdateProfile {
                name: Some("Jarvis".into()),
                description: None,
                avatar: Some("sun".into()),
                personality: Some(Template::Formal.text().into()),
            }
        );
        // The core's answer becomes the stored profile: nothing is left to save.
        let profile = Profile {
            name: "Jarvis".into(),
            avatar: "sun".into(),
            personality: Template::Formal.text().into(),
            ..stored()
        };
        answered(&mut state, &Ok(Response::Profile { profile }));
        assert!(matches!(update(&mut state, Message::Save, &data), Action::None));
    }

    #[test]
    fn problems_show_after_the_first_save_and_nothing_is_sent() {
        let data = data();
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenProfile, &data);
        let _ = update(&mut state, Message::Name("  ".into()), &data);
        let draft = state.draft.as_ref().expect("a draft");
        assert!(!draft.tried && draft.problem() == Some(tr("Give the assistant a name.").to_string()));
        drop(view(&state, &data));
        assert!(matches!(update(&mut state, Message::Save, &data), Action::None));
        assert!(state.draft.as_ref().is_some_and(|draft| draft.tried), "shown from now on");
        drop(view(&state, &data));

        let _ = update(&mut state, Message::Name("Torro".into()), &data);
        let long = "a".repeat(MAX_PERSONALITY_CHARS + 1);
        let mut draft = Draft::new(&Profile { personality: long, ..stored() });
        draft.tried = true;
        assert_eq!(
            draft.problem(),
            Some(trf(
                "The personality is too long: {count} of {max} characters.",
                &[("count", &(MAX_PERSONALITY_CHARS + 1)), ("max", &MAX_PERSONALITY_CHARS)]
            ))
        );
    }

    #[test]
    fn going_back_asks_before_unsaved_changes_are_lost_and_new_notes_take_the_focus() {
        let data = data();
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenProfile, &data);
        let _ = update(&mut state, Message::Back, &data);
        assert!(state.sub.is_none(), "nothing typed: back at once");
        let _ = update(&mut state, Message::OpenProfile, &data);
        let _ = update(&mut state, Message::Name("Jarvis".into()), &data);
        let _ = update(&mut state, Message::Back, &data);
        assert_eq!(state.sub, Some(Sub::Profile), "asks first");
        assert!(overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::CloseDialog)));
        let _ = update(&mut state, Message::CloseDialog, &data);
        assert!(overlay(&state, &data).is_none() && state.draft.is_some(), "still typed");
        let _ = update(&mut state, Message::Back, &data);
        let _ = update(&mut state, Message::Leave, &data);
        assert!(state.sub.is_none() && overlay(&state, &data).is_none());

        let _ = update(&mut state, Message::OpenMemory, &data);
        assert!(matches!(update(&mut state, Message::AddNote, &data), Action::Focus(NOTE_FIELD)));
        assert!(matches!(update(&mut state, Message::EditNote("1a2b3c4d".into()), &data), Action::Focus(NOTE_FIELD)));
        assert!(matches!(update(&mut state, Message::EditNote("ffffffff".into()), &data), Action::None));
    }

    #[test]
    fn discarding_brings_back_what_is_stored_and_a_reload_follows_while_nothing_is_typed() {
        let data = data();
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenProfile, &data);
        let _ = update(&mut state, Message::Description("Mine".into()), &data);
        let _ = update(&mut state, Message::Discard, &data);
        assert!(state.draft.as_ref().is_some_and(|draft| draft.changes().is_none()));

        let mut changed = data.clone();
        changed.profile = Some(Profile { name: "Jarvis".into(), ..stored() });
        loaded(&mut state, &changed);
        assert_eq!(state.draft.as_ref().map(|draft| draft.name.as_str()), Some("Jarvis"), "follows");
        let _ = update(&mut state, Message::Name("Friday".into()), &changed);
        loaded(&mut state, &data);
        assert_eq!(state.draft.as_ref().map(|draft| draft.name.as_str()), Some("Friday"), "what is typed stays");
    }

    #[test]
    fn notes_are_added_changed_and_forgotten_with_requests() {
        let data = data();
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenMemory, &data);
        let _ = update(&mut state, Message::AddNote, &data);
        assert!(matches!(update(&mut state, Message::SaveNote, &data), Action::None), "empty: nothing sent");
        let _ = update(&mut state, Message::NoteInput(" Bob likes tea. ".into()), &data);
        assert_eq!(
            sent(update(&mut state, Message::SaveNote, &data)),
            Request::Remember { text: "Bob likes tea.".into(), replaces: None, local_only: false }
        );
        let created = note("9a9a9a9a", "Bob likes tea.", false);
        answered(&mut state, &Ok(Response::Memory { note: created }));
        assert!(state.edit.is_none(), "stored: the editor closes");

        let _ = update(&mut state, Message::EditNote("1a2b3c4d".into()), &data);
        assert_eq!(state.edit.as_ref().map(|edit| edit.text.as_str()), Some("Anna works in sales."));
        let _ = update(&mut state, Message::NoteInput("Anna works in marketing.".into()), &data);
        assert_eq!(
            sent(update(&mut state, Message::SaveNote, &data)),
            Request::Remember {
                text: "Anna works in marketing.".into(),
                replaces: Some("1a2b3c4d".into()),
                local_only: false
            }
        );
        assert!(matches!(escape(&state), Some(Message::CancelNote)));
        let _ = update(&mut state, Message::CancelNote, &data);

        let _ = update(&mut state, Message::AskForget("5e6f7a8b".into()), &data);
        assert!(overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::CloseDialog)));
        assert_eq!(sent(update(&mut state, Message::Forget, &data)), Request::Forget { id: "5e6f7a8b".into() });
        assert!(overlay(&state, &data).is_none());
        let _ = update(&mut state, Message::AskForget("ffffffff".into()), &data);
        assert!(overlay(&state, &data).is_none(), "no such note, no question");
    }

    #[test]
    fn the_pages_build_with_their_headers_in_every_state() {
        let data = data();
        let mut state = State::default();
        assert!(view(&state, &data).is_none() && header(&state, SizeClass::Wide).is_none());
        drop(profile_rows(&data));
        drop(profile_rows(&Data::default()));
        let _ = update(&mut state, Message::OpenProfile, &data);
        assert!(view(&state, &data).is_some());
        let shown = header(&state, SizeClass::Wide).and_then(|header| header.sub_page).map(|(name, _)| name);
        assert_eq!(shown, Some(tr("Profile").to_string()));
        let _ = update(&mut state, Message::OpenMemory, &data);
        for class in [SizeClass::Wide, SizeClass::Narrow] {
            let memory = header(&state, class).expect("a header");
            assert_eq!(memory.actions.len(), 1, "Add note");
        }
        drop(view(&state, &data));
        drop(view(&state, &Data::default()));
        let _ = update(&mut state, Message::AddNote, &data);
        drop(view(&state, &data));
        let _ = update(&mut state, Message::Back, &data);
        assert!(state.sub.is_none());
    }

    #[test]
    fn every_picture_has_a_word_and_unknown_ones_are_the_letter() {
        for id in AVATARS {
            let (label, mark) = avatar_choice(id);
            assert!(!label.is_empty());
            assert_eq!(mark.is_none(), *id == LETTER_AVATAR, "{id}");
        }
        assert!(avatar_icon("dragon").is_none());
        drop(avatar::<Message>("torro", "dragon", size::control_sm()));
        drop(avatar::<Message>("", LETTER_AVATAR, size::control_lg()));
    }

    #[test]
    fn templates_are_starting_points_with_words() {
        for template in Template::ALL {
            assert!(!template.label().is_empty() && template.text().chars().count() <= MAX_PERSONALITY_CHARS);
        }
        assert_eq!(Template::Calm.label(), tr("Calm and brief"));
    }
}
