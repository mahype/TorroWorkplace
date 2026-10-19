//! Assistant › Profile (task S.17; it was Assistant › Profile and Assistant › Memory of the
//! control centre, task C.10, concept `docs/assistent.md`, "Persönlichkeit (Stufe 1)"): who the
//! assistant is and what it keeps because the person asked it to.
//!
//! The section's own page holds two rows; each opens a page below it ([`crate::Page::sub_page`]):
//!
//! - **Profile:** name, short description, picture (the first letter of the name or one of the
//!   suite's symbols, [`torro_ipc::assistant::AVATARS`]) and the personality, typed into a draft
//!   and saved with "Save" (`assistant.update_profile`, only the fields that changed). Three
//!   templates ("Calm and brief", "Friendly", "Formal") fill the personality as a starting point;
//!   "Discard changes" brings back what is stored. A blank name and a personality over 4,000
//!   characters are named after the first "Save", not before.
//! - **Memory:** the notes (`assistant.list_memories`), each with "Edit" (`assistant.remember` with
//!   `replaces`) and "Forget" (`assistant.forget`, after asking); "Add note" in the header bar.
//!   A note kept from models outside this computer says so ("This computer only").
//!
//! The page reads and writes nothing itself: the profile and the notes come with [`Data`], every
//! change is a request the host sends. **A section never sees the answer** – the host sends and
//! loads again –, so the draft measures itself against the stored profile of the fresh [`Data`]:
//! once a change is stored, "Save" greys out by itself, and what is still typed stays typed. A
//! note being written closes as it is sent; if it fails, the host's message stands over the page.
//!
//! A typed profile is never dropped without asking: the back button of the page asks
//! "Discard changes?", and so does every way out of the settings themselves – the window asks
//! [`closing`] first and lets go only after "Discard" ([`crate::Action::Close`]).

use chrono::{NaiveDate, Utc};
use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, svg, text_editor};
use iced::{Alignment, Element, Fill, Theme};

use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_short_date, to_local};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Mark};
use torro_gui::{style, typo};
use torro_ipc::Request;
use torro_ipc::assistant::{
    AVATARS, DEFAULT_NAME, LETTER_AVATAR, MAX_NOTE_CHARS, MAX_PERSONALITY_CHARS, Note, Profile,
};

use super::{Action, avatar};
use crate::parts::{chevron, confirm, field, pressable, row_frame, setting_row};
use crate::{Data, Page};

/// The id of the field a note is typed into: it takes the focus when "Add note" or "Edit" opens it
/// ([`crate::Action::Focus`]).
pub const NOTE_FIELD: &str = "assistant-note";

/// The page below the section that is open.
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

/// Section state.
#[derive(Debug, Default)]
pub struct State {
    /// The page below the section that is open; `None` on its own page.
    pub sub: Option<Sub>,
    /// What is typed into the profile; made when its page opens.
    draft: Option<Draft>,
    /// A note being written (`id: None`) or changed.
    edit: Option<NoteEdit>,
    /// "Forget this note?" for the note with this id.
    forget: Option<String>,
    /// "Discard changes?" while it is open, and what pressing "Discard" then leads to.
    leaving: Option<Leaving>,
}

/// What a "Discard" in "Discard changes?" lets go of: the unsaved profile is dropped either way,
/// the question only differs in how far the way out leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leaving {
    /// The back button in the header: back to the section's own two rows.
    Page,
    /// The window wants its content area back – the gear, Escape, a click in the sidebar: the
    /// settings close ([`crate::Action::Close`]).
    Settings,
}

/// The profile as it is being typed.
#[derive(Debug)]
struct Draft {
    name: String,
    description: String,
    avatar: String,
    personality: text_editor::Content,
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
            tried: false,
        }
    }

    /// The personality as it would be sent: line breaks as `\n`, no white space at the ends.
    fn personality_text(&self) -> String {
        self.personality.text().replace("\r\n", "\n").trim().to_string()
    }

    /// What changed against `stored`, as `assistant.update_profile`; `None` when nothing did.
    fn changes(&self, stored: &Profile) -> Option<Request> {
        let changed = |typed: &str, stored: &str| (typed.trim() != stored.trim()).then(|| typed.trim().to_string());
        let request = Request::UpdateProfile {
            name: changed(&self.name, &stored.name),
            description: changed(&self.description, &stored.description),
            avatar: (self.avatar != stored.avatar).then(|| self.avatar.clone()),
            personality: changed(&self.personality_text(), &stored.personality),
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
    /// The two rows of the section's own page.
    OpenProfile,
    OpenMemory,
    /// The back button in the header: asks first while the profile has unsaved changes.
    Back,
    /// The window wants its content area back – the gear, Escape, a click in the sidebar – while
    /// the profile has unsaved changes: ask first. The window asks for this message through
    /// [`closing`] instead of closing the settings itself.
    AskClose,
    /// "Discard" in "Discard changes?": on without saving, as far as the way led ([`Leaving`]).
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
    /// "Add note" in the header bar.
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

/// The profile the daemon holds, or the defaults before it answered.
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
        Message::Back if unsaved(state, data) => {
            state.leaving = Some(Leaving::Page);
            Action::None
        }
        Message::Back => {
            *state = State::default();
            Action::None
        }
        Message::AskClose => {
            state.leaving = Some(Leaving::Settings);
            Action::None
        }
        Message::Leave => {
            // The way out is taken now that it was confirmed: the typed profile goes with the
            // state, and a way that led out of the settings takes them with it.
            let leaving = state.leaving;
            *state = State::default();
            match leaving {
                Some(Leaving::Settings) => Action::Close,
                Some(Leaving::Page) | None => Action::None,
            }
        }
        Message::Name(name) => edit_draft(state, |draft| draft.name = name),
        Message::Description(description) => edit_draft(state, |draft| draft.description = description),
        Message::Avatar(avatar) => edit_draft(state, |draft| draft.avatar = avatar),
        Message::Personality(action) => edit_draft(state, |draft| draft.personality.perform(action)),
        Message::Template(template) => {
            edit_draft(state, |draft| draft.personality = text_editor::Content::with_text(template.text()))
        }
        Message::Save => save(state, data),
        Message::Discard => {
            state.draft = Some(Draft::new(&profile_of(data)));
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
            Some(id) => Action::send(Request::Forget { id }),
            None => Action::None,
        },
        Message::CloseDialog => {
            state.forget = None;
            state.leaving = None;
            Action::None
        }
    }
}

/// Whether the profile holds changes that are not stored yet.
fn unsaved(state: &State, data: &Data) -> bool {
    state.draft.as_ref().is_some_and(|draft| draft.changes(&profile_of(data)).is_some())
}

/// Changes the draft, when there is one.
fn edit_draft(state: &mut State, change: impl FnOnce(&mut Draft)) -> Action {
    if let Some(draft) = &mut state.draft {
        change(draft);
    }
    Action::None
}

/// "Save": what changed, unless a problem keeps it back (shown from now on).
fn save(state: &mut State, data: &Data) -> Action {
    let stored = profile_of(data);
    let Some(draft) = &mut state.draft else { return Action::None };
    draft.tried = true;
    if draft.problem().is_some() {
        return Action::None;
    }
    draft.changes(&stored).map_or(Action::None, Action::send)
}

/// "Save" at a note: `assistant.remember`, with `replaces` for a changed note. The editor closes
/// as it is sent: the section never sees the answer, and the host shows what failed.
fn save_note(state: &mut State) -> Action {
    let Some(edit) = &mut state.edit else { return Action::None };
    edit.tried = true;
    if edit.problem().is_some() {
        return Action::None;
    }
    let request =
        Request::Remember { text: edit.text.trim().to_string(), replaces: edit.id.clone(), local_only: false };
    state.edit = None;
    Action::send(request)
}

/// What stands between the window and closing these settings: the question "Discard changes?"
/// while the typed profile is not stored. `None` lets the window close them.
///
/// The question sits here, not in the window: only this section knows what is typed, it already
/// builds the dialog for the back button, and "Discard" closes the settings from here
/// ([`crate::Action::Close`]) – so all three ways out ask the same question and let go at the same
/// moment.
pub fn closing(state: &State, data: &Data) -> Option<Message> {
    unsaved(state, data).then_some(Message::AskClose)
}

/// What Escape means here: close the question, then stop typing a note.
pub fn escape(state: &State) -> Option<Message> {
    if state.forget.is_some() || state.leaving.is_some() {
        Some(Message::CloseDialog)
    } else if state.edit.is_some() {
        Some(Message::CancelNote)
    } else {
        None
    }
}

/// "Forget this note?" or "Discard changes?" while one is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if state.leaving.is_some() {
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

// ----- Views --------------------------------------------------------------------------------

/// The section's page: its two rows, or the page below it that is open – Profile, or Memory with
/// "Add note" in the header bar (only its icon in narrow windows).
pub fn page<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Page<'a, Message> {
    if !super::is_on(data) {
        return Page::new(super::off_group(tr("Profile")));
    }
    match (state.sub, state.draft.as_ref()) {
        (Some(Sub::Profile), Some(draft)) => {
            Page::new(profile_page(draft, &profile_of(data))).sub_page(tr("Profile"), Message::Back)
        }
        (Some(Sub::Memory), _) => {
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
            Page::new(memory_page(state, data)).sub_page(tr("Memory"), Message::Back).actions(vec![button])
        }
        _ => Page::new(profile_rows(data)),
    }
}

/// The two rows of the section: the profile with its picture, name and description, the memory
/// with how many notes it holds.
fn profile_rows(data: &Data) -> Element<'_, Message> {
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
        icon::icon(Icon::AlignLeft, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_2) }),
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
fn profile_page<'a>(draft: &'a Draft, stored: &Profile) -> Element<'a, Message> {
    let save_on = draft.changes(stored).is_some();
    let name = field(DEFAULT_NAME, &draft.name).on_input(Message::Name).on_submit(Message::Save);
    let description = field(tr("Your assistant for this computer"), &draft.description)
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
fn note_row<'a>(note: &'a Note, today: NaiveDate) -> Element<'a, Message> {
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
    let typed = field(tr("For example: Anna works in sales."), &edit.text)
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
    let mut content = column![typed, buttons].spacing(space::s());
    if let Some(problem) = edit.problem().filter(|_| edit.tried) {
        content = content.push(problem_text(problem));
    }
    inset(content)
}

/// A problem of a form: an alert icon in the error colour and the words in `text`; the colour is
/// only in the icon.
fn problem_text<'a>(problem: String) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE_SM)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
    row![alert, typo::text(typo::meta(), problem)].spacing(space::xs()).align_y(Alignment::Center).into()
}

/// The word and the mark of a picture in the picker.
fn avatar_choice(id: &str) -> (&'static str, Option<Mark>) {
    match super::avatar_icon(id) {
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::super::tests::{data, turned_off};
    use super::*;
    use crate::Action as Act;

    fn note(id: &str, text: &str, local_only: bool) -> Note {
        let created = Utc.with_ymd_and_hms(2026, 10, 14, 9, 0, 0).single().expect("a time");
        Note { id: id.into(), text: text.into(), created, local_only }
    }

    fn sent(action: Action) -> Request {
        match action {
            Act::Send(requests) if requests.len() == 1 => requests.into_iter().next().expect("one"),
            other => panic!("expected one request, got {other:?}"),
        }
    }

    /// The demo's notes, by the ids the tests change and forget.
    async fn with_notes() -> Data {
        let mut data = data().await;
        data.memories =
            vec![note("1a2b3c4d", "Anna works in sales.", false), note("5e6f7a8b", "ACME owes 400 €.", true)];
        data
    }

    #[tokio::test]
    async fn saving_the_profile_sends_only_what_changed() {
        let data = with_notes().await;
        let stored = profile_of(&data);
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenProfile, &data);
        assert_eq!(state.sub, Some(Sub::Profile));
        assert!(matches!(update(&mut state, Message::Save, &data), Act::None), "nothing changed");

        let _ = update(&mut state, Message::Name("Jarvis".into()), &data);
        let _ = update(&mut state, Message::Avatar("sun".into()), &data);
        let _ = update(&mut state, Message::Template(Template::Formal), &data);
        assert_eq!(
            sent(update(&mut state, Message::Save, &data)),
            Request::UpdateProfile {
                name: Some("Jarvis".into()),
                description: None,
                avatar: Some("sun".into()),
                personality: Some(Template::Formal.text().into()),
            }
        );
        // The host loads again; against the stored profile nothing is left to save.
        let saved = Data {
            profile: Some(Profile {
                name: "Jarvis".into(),
                avatar: "sun".into(),
                personality: Template::Formal.text().into(),
                ..stored
            }),
            ..data.clone()
        };
        assert!(matches!(update(&mut state, Message::Save, &saved), Act::None), "stored: Save greys out");
    }

    #[tokio::test]
    async fn problems_show_after_the_first_save_and_nothing_is_sent() {
        let data = with_notes().await;
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenProfile, &data);
        let _ = update(&mut state, Message::Name("  ".into()), &data);
        let draft = state.draft.as_ref().expect("a draft");
        assert!(!draft.tried && draft.problem() == Some(tr("Give the assistant a name.").to_string()));
        let _ = page(&state, &data, SizeClass::Wide);
        assert!(matches!(update(&mut state, Message::Save, &data), Act::None));
        assert!(state.draft.as_ref().is_some_and(|draft| draft.tried), "shown from now on");
        let _ = page(&state, &data, SizeClass::Wide);

        let long = "a".repeat(MAX_PERSONALITY_CHARS + 1);
        let mut draft = Draft::new(&Profile { personality: long, ..profile_of(&data) });
        draft.tried = true;
        assert_eq!(
            draft.problem(),
            Some(trf(
                "The personality is too long: {count} of {max} characters.",
                &[("count", &(MAX_PERSONALITY_CHARS + 1)), ("max", &MAX_PERSONALITY_CHARS)]
            ))
        );
    }

    #[tokio::test]
    async fn going_back_asks_before_unsaved_changes_are_lost_and_new_notes_take_the_focus() {
        let data = with_notes().await;
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
        assert!(matches!(update(&mut state, Message::AddNote, &data), Act::Focus(NOTE_FIELD)));
        assert!(matches!(update(&mut state, Message::EditNote("1a2b3c4d".into()), &data), Act::Focus(NOTE_FIELD)));
        assert!(matches!(update(&mut state, Message::EditNote("ffffffff".into()), &data), Act::None));
    }

    /// The same question for the window's way out (found in the review of #210): while something
    /// is typed, [`closing`] hands the window [`Message::AskClose`] instead of letting it close
    /// the settings, and only "Discard" lets them go – then from here, with [`Act::Close`].
    #[tokio::test]
    async fn closing_the_settings_asks_before_an_unsaved_profile_is_lost() {
        let data = with_notes().await;
        let mut state = State::default();
        assert!(closing(&state, &data).is_none(), "nothing open, nothing to ask");
        let _ = update(&mut state, Message::OpenProfile, &data);
        assert!(closing(&state, &data).is_none(), "nothing typed either");

        let _ = update(&mut state, Message::Name("Jarvis".into()), &data);
        let ask = closing(&state, &data).expect("the window must ask");
        assert!(matches!(ask, Message::AskClose));
        let _ = update(&mut state, ask, &data);
        assert!(overlay(&state, &data).is_some(), "\"Discard changes?\" stands");
        assert!(matches!(escape(&state), Some(Message::CloseDialog)), "Escape cancels it");
        let _ = update(&mut state, Message::CloseDialog, &data);
        assert!(state.draft.is_some() && closing(&state, &data).is_some(), "still typed, still asking");

        let _ = update(&mut state, Message::AskClose, &data);
        assert!(matches!(update(&mut state, Message::Leave, &data), Act::Close), "\"Discard\" closes the settings");
        assert!(state.sub.is_none() && state.draft.is_none(), "and lets go of what was typed");

        // The back button of the page leads only out of the page, not out of the settings.
        let _ = update(&mut state, Message::OpenProfile, &data);
        let _ = update(&mut state, Message::Name("Friday".into()), &data);
        let _ = update(&mut state, Message::Back, &data);
        assert!(matches!(update(&mut state, Message::Leave, &data), Act::None));
    }

    #[tokio::test]
    async fn discarding_brings_back_what_is_stored_and_what_is_typed_stays_over_a_reload() {
        let data = with_notes().await;
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenProfile, &data);
        let _ = update(&mut state, Message::Description("Mine".into()), &data);
        let _ = update(&mut state, Message::Discard, &data);
        assert!(!unsaved(&state, &data), "back to what is stored");

        let _ = update(&mut state, Message::Name("Friday".into()), &data);
        let changed =
            Data { profile: Some(Profile { description: "Elsewhere".into(), ..profile_of(&data) }), ..data.clone() };
        assert_eq!(state.draft.as_ref().map(|draft| draft.name.as_str()), Some("Friday"), "what is typed stays");
        assert!(unsaved(&state, &changed), "and is still offered against the fresh profile");
    }

    #[tokio::test]
    async fn notes_are_added_changed_and_forgotten_with_requests() {
        let data = with_notes().await;
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenMemory, &data);
        let _ = update(&mut state, Message::AddNote, &data);
        assert!(matches!(update(&mut state, Message::SaveNote, &data), Act::None), "empty: nothing sent");
        let _ = update(&mut state, Message::NoteInput(" Bob likes tea. ".into()), &data);
        assert_eq!(
            sent(update(&mut state, Message::SaveNote, &data)),
            Request::Remember { text: "Bob likes tea.".into(), replaces: None, local_only: false }
        );
        assert!(state.edit.is_none(), "sent: the editor closes, the host loads again");

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

        let _ = update(&mut state, Message::EditNote("5e6f7a8b".into()), &data);
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

    #[tokio::test]
    async fn the_pages_build_with_their_headers_in_every_state() {
        let data = with_notes().await;
        let mut state = State::default();
        assert!(page(&state, &data, SizeClass::Wide).sub_page.is_none(), "the section's own rows");
        let _ = page(&state, &Data { memories: Vec::new(), ..data.clone() }, SizeClass::Wide);
        let _ = update(&mut state, Message::OpenProfile, &data);
        let shown = page(&state, &data, SizeClass::Wide).sub_page.map(|(name, _)| name);
        assert_eq!(shown.as_deref(), Some(tr("Profile")));
        let _ = update(&mut state, Message::OpenMemory, &data);
        for class in [SizeClass::Wide, SizeClass::Narrow] {
            let memory = page(&state, &data, class);
            assert_eq!(memory.actions.len(), 1, "Add note");
            assert_eq!(memory.sub_page.map(|(name, _)| name).as_deref(), Some(tr("Memory")));
        }
        let _ = update(&mut state, Message::AddNote, &data);
        let _ = page(&state, &data, SizeClass::Wide);
        // While the assistant is off the section waits, whatever was open.
        let off = turned_off(&data);
        assert!(page(&state, &off, SizeClass::Wide).sub_page.is_none());
    }

    #[test]
    fn templates_are_starting_points_with_words() {
        for template in Template::ALL {
            assert!(!template.label().is_empty() && template.text().chars().count() <= MAX_PERSONALITY_CHARS);
        }
        assert_eq!(Template::Calm.label(), tr("Calm and brief"));
        for id in AVATARS {
            let (label, mark) = avatar_choice(id);
            assert!(!label.is_empty());
            assert_eq!(mark.is_none(), *id == LETTER_AVATAR, "{id}");
        }
        assert_eq!(avatar_label("sun"), tr("Sun"));
    }
}
