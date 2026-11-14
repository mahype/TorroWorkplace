//! Assistant › Skills (issue #703, concept `docs/assistent.md`, "Skills"): guides the person writes
//! for tasks that come again ("How I write offers"). The assistant's prompt names each with its
//! description in one line; it reads the text with `assistant.get_skill` when a task fits.
//!
//! The section's own page lists the skills (`assistant.list_skills`, names and descriptions only),
//! each with "Edit" and "Delete" (`assistant.delete_skill`, after asking); "Add skill" in the header
//! bar. "Edit" fetches the text first (`assistant.get_skill` as an [`Action::Ask`]: the list holds no
//! texts) and opens the page below the section, where name, description and text are typed and
//! saved with the suite's save bar (`assistant.save_skill`, with `replaces` for a skill that is
//! changed). The editor closes as it is sent: the section never sees the answer, and the host shows
//! what failed.
//!
//! A typed skill is never dropped without asking: the back button asks "Discard changes?", and so
//! does every way out of the settings ([`closing`]).

use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, svg, text_editor};
use iced::{Alignment, Element, Fill, Theme};

use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, space};
use torro_gui::{style, typo, widgets};
use torro_ipc::assistant::{
    MAX_SKILL_CHARS, MAX_SKILL_DESCRIPTION_CHARS, MAX_SKILL_NAME_CHARS, MAX_SKILLS, Skill, same_skill_name,
};
use torro_ipc::{Request, Response};

use super::Action;
use crate::parts::{confirm, field, setting_row};
use crate::{Data, Page};

/// The id of the name's field: it takes the focus when "Add skill" opens the editor.
pub const NAME_FIELD: &str = "assistant-skill-name";

/// Section state.
#[derive(Debug, Default)]
pub struct State {
    /// The skill being written or changed; `None` on the section's own page.
    draft: Option<Draft>,
    /// "Edit" asked for this skill's text and waits for it ([`answered`]).
    fetching: Option<String>,
    /// "Delete this skill?" for the skill with this name.
    delete: Option<String>,
    /// "Discard changes?" while it is open, and how far "Discard" then leads.
    leaving: Option<Leaving>,
}

/// What "Discard" in "Discard changes?" lets go of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leaving {
    /// The back button: back to the list.
    Page,
    /// The window wants its content area back: the settings close.
    Settings,
}

/// A skill as it is being typed.
#[derive(Debug)]
struct Draft {
    /// The skill as stored, for a changed one; `None` for a new one.
    stored: Option<Skill>,
    name: String,
    description: String,
    text: text_editor::Content,
    /// "Save" was pressed once: problems show from now on.
    tried: bool,
}

impl Draft {
    fn new(stored: Option<Skill>) -> Draft {
        let (name, description, text) = match &stored {
            Some(skill) => (skill.name.clone(), skill.description.clone(), skill.text.clone()),
            None => Default::default(),
        };
        Draft { stored, name, description, text: text_editor::Content::with_text(&text), tried: false }
    }

    /// The text as it would be sent: line breaks as `\n`, no white space at the ends.
    fn text(&self) -> String {
        self.text.text().replace("\r\n", "\n").trim().to_string()
    }

    /// Whether anything differs from the stored skill (for a new one: anything typed).
    fn changed(&self) -> bool {
        let (name, description, text) = match &self.stored {
            Some(skill) => (skill.name.as_str(), skill.description.as_str(), skill.text.as_str()),
            None => ("", "", ""),
        };
        self.name.trim() != name.trim() || self.description.trim() != description.trim() || self.text() != text.trim()
    }

    /// What keeps "Save" from sending, for people; `None` when it can. A name another skill has is
    /// the core's to refuse (`conflict`), but the list knows them, so it says so first.
    fn problem(&self, data: &Data) -> Option<String> {
        let name = self.name.trim();
        let count = self.text().chars().count();
        let kept = self.stored.as_ref().map(|skill| skill.name.as_str());
        if name.is_empty() {
            Some(tr("Give the skill a name.").to_string())
        } else if name.chars().count() > MAX_SKILL_NAME_CHARS {
            Some(trf("A name holds at most {max} characters.", &[("max", &MAX_SKILL_NAME_CHARS)]))
        } else if data
            .skills
            .iter()
            .any(|skill| same_skill_name(&skill.name, name) && kept != Some(skill.name.as_str()))
        {
            Some(tr("Another skill has this name.").to_string())
        } else if self.description.trim().chars().count() > MAX_SKILL_DESCRIPTION_CHARS {
            Some(trf("A description holds at most {max} characters.", &[("max", &MAX_SKILL_DESCRIPTION_CHARS)]))
        } else if count == 0 {
            Some(tr("Write the skill first.").to_string())
        } else if count > MAX_SKILL_CHARS {
            Some(trf(
                "The skill is too long: {count} of {max} characters.",
                &[("count", &count), ("max", &MAX_SKILL_CHARS)],
            ))
        } else {
            None
        }
    }

    /// `assistant.save_skill` as typed.
    fn request(&self) -> Request {
        Request::SaveSkill {
            name: self.name.trim().to_string(),
            description: self.description.trim().to_string(),
            text: self.text(),
            replaces: self.stored.as_ref().map(|skill| skill.name.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// "Add skill" in the header bar.
    Add,
    /// "Edit" at the skill with this name: fetches its text first.
    Edit(String),
    Name(String),
    Description(String),
    Text(text_editor::Action),
    /// "Save" in the save bar, Enter in a field.
    Save,
    /// "Discard" in the save bar: back to the stored skill (a new one: empty).
    Discard,
    /// The back button in the header: asks first while the skill has unsaved changes.
    Back,
    /// The window wants its content area back while the skill has unsaved changes: ask first.
    AskClose,
    /// "Discard" in "Discard changes?".
    Leave,
    /// "Delete" at the skill with this name: asks first.
    AskDelete(String),
    Delete,
    CloseDialog,
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Add => {
            state.draft = Some(Draft::new(None));
            Action::Focus(NAME_FIELD)
        }
        Message::Edit(name) => {
            state.fetching = Some(name.clone());
            Action::Ask(Request::GetSkill { name })
        }
        Message::Name(name) => edit_draft(state, |draft| draft.name = name),
        Message::Description(description) => edit_draft(state, |draft| draft.description = description),
        Message::Text(action) => edit_draft(state, |draft| draft.text.perform(action)),
        Message::Save => save(state, data),
        Message::Discard => {
            if let Some(draft) = &mut state.draft {
                *draft = Draft::new(draft.stored.take());
            }
            Action::None
        }
        Message::Back if unsaved(state) => {
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
            let leaving = state.leaving;
            *state = State::default();
            match leaving {
                Some(Leaving::Settings) => Action::Close,
                Some(Leaving::Page) | None => Action::None,
            }
        }
        Message::AskDelete(name) => {
            state.delete = data.skills.iter().any(|skill| skill.name == name).then_some(name);
            Action::None
        }
        Message::Delete => match state.delete.take() {
            Some(name) => Action::send(Request::DeleteSkill { name }),
            None => Action::None,
        },
        Message::CloseDialog => {
            state.delete = None;
            state.leaving = None;
            Action::None
        }
    }
}

/// The answer to "Edit"'s `assistant.get_skill` ([`crate::Outcome::Answer`]): the skill it waited
/// for opens in the editor; any other answer is left alone, and a failure the host shows.
pub fn answered(state: &mut State, result: &Result<Response, String>) {
    let Some(wanted) = &state.fetching else { return };
    match result {
        Ok(Response::Skill { skill }) if same_skill_name(&skill.name, wanted) => {
            state.fetching = None;
            state.draft = Some(Draft::new(Some(skill.clone())));
        }
        Err(_) | Ok(Response::Error { .. }) => state.fetching = None,
        Ok(_) => {}
    }
}

/// Whether the skill being typed holds changes that are not stored yet.
fn unsaved(state: &State) -> bool {
    state.draft.as_ref().is_some_and(Draft::changed)
}

/// Changes the draft, when there is one.
fn edit_draft(state: &mut State, change: impl FnOnce(&mut Draft)) -> Action {
    if let Some(draft) = &mut state.draft {
        change(draft);
    }
    Action::None
}

/// "Save": `assistant.save_skill`, unless a problem keeps it back (shown from now on); the editor
/// closes as it is sent.
fn save(state: &mut State, data: &Data) -> Action {
    let Some(draft) = &mut state.draft else { return Action::None };
    draft.tried = true;
    if !draft.changed() || draft.problem(data).is_some() {
        return Action::None;
    }
    let request = draft.request();
    state.draft = None;
    Action::send(request)
}

/// The question "Discard changes?" while a typed skill is not stored. `None` lets the window close
/// the settings.
pub fn closing(state: &State) -> Option<Message> {
    unsaved(state).then_some(Message::AskClose)
}

/// What Escape means here: close the question. Typing is left with the back button, which asks.
pub fn escape(state: &State) -> Option<Message> {
    (state.delete.is_some() || state.leaving.is_some()).then_some(Message::CloseDialog)
}

/// "Delete this skill?" or "Discard changes?" while one is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if state.leaving.is_some() {
        return Some(confirm(
            tr("Discard changes?"),
            tr("What you changed in the skill is not saved.").to_string(),
            tr("Discard"),
            Message::Leave,
            Message::CloseDialog,
        ));
    }
    let name = state.delete.as_ref()?;
    let skill = data.skills.iter().find(|skill| &skill.name == name)?;
    Some(confirm(
        tr("Delete this skill?"),
        trf("The assistant will no longer know “{name}”. This cannot be undone.", &[("name", &skill.name)]),
        tr("Delete"),
        Message::Delete,
        Message::CloseDialog,
    ))
}

/// Whether `message` leaves the page below the section (the back button), for the host's menu.
pub fn leaves_page(message: &Message) -> bool {
    matches!(message, Message::Back)
}

// ----- Views --------------------------------------------------------------------------------

/// The section's page: the list with "Add skill" in the header bar (only its icon in narrow
/// windows), or the editor below it.
pub fn page<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Page<'a, Message> {
    if !super::is_on(data) {
        return Page::new(super::off_group(tr("Skills")));
    }
    if let Some(draft) = &state.draft {
        let title = match &draft.stored {
            Some(skill) => skill.name.clone(),
            None => tr("New skill").to_string(),
        };
        let page = Page::new(editor(draft)).sub_page(title, Message::Back);
        return match draft.changed() {
            true => {
                let summary =
                    draft.problem(data).filter(|_| draft.tried).unwrap_or_else(|| tr("Skill changed").to_string());
                page.save_bar(widgets::save_bar(summary, Some(Message::Discard), Some(Message::Save), false))
            }
            false => page,
        };
    }
    let label = tr("Add skill");
    let on_press = (data.skills.len() < MAX_SKILLS).then_some(Message::Add);
    let button = match class {
        SizeClass::Wide | SizeClass::Medium => {
            widgets::text_button(Some(Icon::Plus), label, style::button::primary, on_press)
        }
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
            widgets::compact_button(Icon::Plus, label, style::button::primary, on_press)
        }
    };
    Page::new(list(state, data)).actions(vec![button])
}

/// The skills by name, each with its description, "Edit" and "Delete".
fn list<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = data
        .skills
        .iter()
        .map(|skill| {
            let fetching = state.fetching.as_deref().is_some_and(|name| same_skill_name(name, &skill.name));
            let edit = (!fetching).then(|| Message::Edit(skill.name.clone()));
            let actions = row![
                widgets::icon_button(Icon::SquarePen, tr("Edit"), edit),
                widgets::icon_button(Icon::Trash2, tr("Delete"), Some(Message::AskDelete(skill.name.clone()))),
            ]
            .spacing(space::xs());
            let description = Some(skill.description.clone()).filter(|text| !text.is_empty());
            setting_row(Some(mark()), skill.name.as_str(), description, actions)
        })
        .collect();
    if rows.is_empty() {
        let hint = tr("Add a guide for a task that comes again, such as how you write offers.");
        rows.push(setting_row(None, tr("No skills yet"), Some(hint.to_string()), Space::new()));
    }
    let footer = tr(
        "The assistant sees the name and description of each skill and reads its text when a task fits. A skill never changes what the assistant may use, and the rules of Torro Workplace always come first.",
    );
    widgets::group(tr("Skills"), rows, Some(footer.to_string()))
}

/// The mark of a skill's row: a light bulb in `text_2`, in a box as wide as a picture.
fn mark<'a>() -> Element<'a, Message> {
    container(
        icon::icon(Icon::Lightbulb, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_2) }),
    )
    .center(theme::size::control_sm())
    .into()
}

/// The text's field shows this many lines before it scrolls.
const TEXT_LINES: f32 = 14.0;

/// The room of the text's field: [`TEXT_LINES`] of text and the padding of a field.
fn text_height() -> f32 {
    TEXT_LINES * typo::line_height(typo::body()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// "Skill" (name, description) and "Text" with its counter; "Save" is the page's save bar.
fn editor(draft: &Draft) -> Element<'_, Message> {
    let name = field(tr("Offers"), &draft.name).id(NAME_FIELD).on_input(Message::Name).on_submit(Message::Save);
    let description =
        field(tr("How I write offers"), &draft.description).on_input(Message::Description).on_submit(Message::Save);
    let when = tr("Says when the skill fits. The assistant sees it in every chat.").to_string();
    let head = vec![
        widgets::segment_row(tr("Name"), None, name, None),
        widgets::segment_row(tr("Description"), Some(when), description, None),
    ];
    let text = text_editor(&draft.text)
        .on_action(Message::Text)
        .placeholder(tr("What the assistant does, step by step"))
        .font(typo::body().font)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .padding(style::INPUT_PADDING)
        .wrapping(Wrapping::WordOrGlyph)
        .height(text_height())
        .style(style::text_editor);
    let count = draft.text().chars().count();
    let counter = typo::text(
        typo::meta().tabular(),
        trf("{count} of {max} characters", &[("count", &count), ("max", &MAX_SKILL_CHARS)]),
    )
    .style(theme::text_3);
    let body = container(column![text, row![blank::horizontal(), counter]].spacing(space::s()))
        .width(Fill)
        .padding([space::m(), space::l()]);
    column![
        widgets::group(tr("Skill"), head, None),
        widgets::group(
            tr("Text"),
            vec![body.into()],
            Some(tr("Markdown or plain text. The assistant reads it when the description fits.").to_string()),
        ),
    ]
    .spacing(layout::section_gap())
    .align_x(Alignment::Start)
    .into()
}

#[cfg(test)]
mod tests {
    use torro_ipc::assistant::SkillSummary;

    use super::super::tests::data;
    use super::*;
    use crate::Action as Act;

    fn sent(action: Action) -> Request {
        match action {
            Act::Send(requests) if requests.len() == 1 => requests.into_iter().next().expect("one"),
            other => panic!("expected one request, got {other:?}"),
        }
    }

    /// What typing `text` into the skill's text sends.
    fn paste(text: &str) -> text_editor::Action {
        text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(text.into())))
    }

    fn skill(name: &str, text: &str) -> Skill {
        Skill { name: name.into(), description: "How I write offers".into(), text: text.into() }
    }

    #[tokio::test]
    async fn a_new_skill_is_saved_once_it_has_a_name_and_a_text() {
        let data = data().await;
        assert!(!data.skills.is_empty(), "the demo has skills");
        let mut state = State::default();
        assert_eq!(update(&mut state, Message::Add, &data), Act::Focus(NAME_FIELD));
        assert_eq!(update(&mut state, Message::Save, &data), Act::None, "nothing typed");
        let _ = update(&mut state, Message::Name(" Travel ".into()), &data);
        assert_eq!(update(&mut state, Message::Save, &data), Act::None, "no text yet");
        let problem = state.draft.as_ref().and_then(|draft| draft.problem(&data));
        assert_eq!(problem.as_deref(), Some(tr("Write the skill first.")));
        let _ = update(&mut state, Message::Text(paste("Trains first.")), &data);
        assert_eq!(
            sent(update(&mut state, Message::Save, &data)),
            Request::SaveSkill {
                name: "Travel".into(),
                description: String::new(),
                text: "Trains first.".into(),
                replaces: None
            }
        );
        assert!(state.draft.is_none(), "the editor closes as it is sent");

        // A name another skill has is said before the core refuses it.
        let taken = data.skills[0].name.to_uppercase();
        let _ = update(&mut state, Message::Add, &data);
        let _ = update(&mut state, Message::Name(taken), &data);
        let _ = update(&mut state, Message::Text(paste("x")), &data);
        assert_eq!(update(&mut state, Message::Save, &data), Act::None);
        let problem = state.draft.as_ref().and_then(|draft| draft.problem(&data));
        assert_eq!(problem.as_deref(), Some(tr("Another skill has this name.")));
    }

    #[tokio::test]
    async fn edit_fetches_the_text_and_saves_with_replaces() {
        let mut data = data().await;
        data.skills = vec![SkillSummary { name: "Offers".into(), description: "How I write offers".into() }];
        let mut state = State::default();
        assert_eq!(
            update(&mut state, Message::Edit("Offers".into()), &data),
            Act::Ask(Request::GetSkill { name: "Offers".into() })
        );
        answered(&mut state, &Ok(Response::Ok));
        assert!(state.draft.is_none() && state.fetching.is_some(), "another answer is left alone");
        answered(&mut state, &Ok(Response::Skill { skill: skill("Offers", "Be brief.") }));
        assert!(!unsaved(&state), "opened as stored");
        assert_eq!(update(&mut state, Message::Save, &data), Act::None, "nothing changed");
        let _ = update(&mut state, Message::Name("Offers and quotes".into()), &data);
        assert!(closing(&state).is_some(), "a way out asks first");
        assert_eq!(
            sent(update(&mut state, Message::Save, &data)),
            Request::SaveSkill {
                name: "Offers and quotes".into(),
                description: "How I write offers".into(),
                text: "Be brief.".into(),
                replaces: Some("Offers".into()),
            }
        );

        // Back with changes asks; "Discard" leaves.
        let _ = update(&mut state, Message::Edit("Offers".into()), &data);
        answered(&mut state, &Ok(Response::Skill { skill: skill("Offers", "Be brief.") }));
        let _ = update(&mut state, Message::Description("Other".into()), &data);
        assert_eq!(update(&mut state, Message::Back, &data), Act::None);
        assert!(overlay(&state, &data).is_some() && escape(&state).is_some());
        assert_eq!(update(&mut state, Message::Leave, &data), Act::None);
        assert!(state.draft.is_none());
    }

    #[tokio::test]
    async fn delete_asks_first() {
        let data = data().await;
        let name = data.skills[0].name.clone();
        let mut state = State::default();
        let _ = update(&mut state, Message::AskDelete(name.clone()), &data);
        assert!(overlay(&state, &data).is_some());
        assert_eq!(sent(update(&mut state, Message::Delete, &data)), Request::DeleteSkill { name });
        assert!(overlay(&state, &data).is_none());
    }
}
