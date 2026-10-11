//! The detail of one task (K.15 in `docs/tasks/kalender-aufgaben.md`, Linear look from T.4 in
//! `docs/tasks/design.md`, mock-up `docs/prototypes/linear/tasks.html`), shown in the side panel
//! of `torro_gui::panel` (U.5): beside the list in the wide layout, over it in the medium and
//! narrow ones.
//!
//! The panel ([`Detail::panel`]): the path "List › Filter" in its head, the title as a large
//! borderless field (`HEADLINE`, wraps), the notes as a quiet field, the properties as rows
//! (the same in every size), the subtasks (round check to tick them, a click on the title opens
//! them) and, fixed in the footer, "Delete task" (`danger`, asks once).
//!
//! Properties: status (open/done), priority (None, Low = 9, Medium = 5, High = 1; stored values
//! map to the nearest), due date, list and parent task. A click on a property opens its editor
//! as a floating menu or card ([`Detail::layer`]); the due card offers Today, Tomorrow, Next week
//! and No date as chips and a field for any date (`11.10.2026`, `today`, `tomorrow`).
//!
//! Saving works like Linear: there is no Save button. A change of a property is saved at once;
//! title and notes are saved a moment after typing stops ([`Detail::wants_autosave`]), on Enter
//! in the title and when the detail is left ([`Detail::leave`]). Every save is the same
//! [`Action::Save`] the panel always sent (the core's `save_todo`). Only one save is under way at
//! a time: the server checks the etag, so the next save has to wait for the new one
//! ([`Detail::stored`]). A title that is empty or a date that cannot be read is not saved; the
//! hint appears only after a save was asked for (Enter, a property change, leaving).

use std::collections::HashSet;

use chrono::{Days, Local, NaiveDate};
use iced::widget::text::Wrapping;
use iced::widget::{button, checkbox, column, container, row, rule, scrollable, text_editor, text_input};
use iced::{Alignment, Background, Color, Element, Fill, Padding, Theme, border, keyboard};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::panel::{self, Panel};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail};
use torro_gui::{datetime, style, typo};
use torro_pim::{Collection, EventTime, Todo};

use crate::list;

/// The id of the date field in the due card, so opening the card can focus it.
pub const DUE_INPUT: &str = "detail-due";
/// The one-pixel lines of the design.
const LINE: f32 = 1.0;
/// The parent menu shows at most this many entries and scrolls the rest.
const PARENT_MENU_ROWS: f32 = 10.0;

/// The panel's state.
#[derive(Debug)]
pub struct Detail {
    /// The task as last stored; the base for the next save (keeps uid, href and etag).
    pub original: Todo,
    /// The writable task lists, for moving the task.
    pub lists: Vec<Collection>,
    /// The other tasks, for the parent menu and the subtasks.
    pub others: Vec<Todo>,
    /// Today, for the due keywords and chips.
    today: NaiveDate,
    draft: Draft,
    /// The title editor; `draft.summary` mirrors its text on one line.
    title: text_editor::Content,
    /// The notes editor; `draft.notes` mirrors its text.
    notes: text_editor::Content,
    /// The date typed into the due card. It can be unfinished or invalid; then the draft keeps
    /// its last valid due and saving is blocked.
    due_text: String,
    /// The property editor that is open.
    open: Option<Editor>,
    /// Set by the first press on Delete; the second press confirms.
    confirm_delete: bool,
    /// A save is under way; the next one waits for its answer (new etag).
    saving: bool,
    /// The last task handed out to be saved, so an unchanged draft is not sent again.
    sent: Option<Todo>,
    /// A save was asked for: from now on problems are shown.
    attempted: bool,
}

/// The property editors that open as a floating layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editor {
    Priority,
    Due,
    List,
    Parent,
}

#[derive(Debug, Clone)]
pub enum Message {
    TitleEdited(text_editor::Action),
    /// Enter in the title: save now.
    TitleSubmit,
    NotesEdited(text_editor::Action),
    /// Save title and notes if they changed (the autosave tick, Escape in a field).
    Flush,
    /// Tick or untick the task itself.
    ToggleDone,
    /// Open a property editor; a second click closes it.
    Open(Editor),
    /// Close the open editor (click beside it).
    CloseEditor,
    PrioritySelected(Priority),
    /// A [`Collection::id`].
    ListSelected(String),
    /// The parent's uid, `None` for none.
    ParentSelected(Option<String>),
    DueChanged(String),
    /// Enter in the date field: take the date and close the card.
    DueSubmit,
    DueToday,
    DueTomorrow,
    DueNextWeek,
    DueRemove,
    /// Tick or untick the subtask with this uid.
    ToggleSubtask(String),
    /// Open the subtask with this uid in the detail.
    OpenSubtask(String),
    Delete,
    CancelDelete,
    ConfirmDelete,
    /// Close the detail.
    Close,
    /// Escape: closes the innermost open thing (editor, delete question, then the detail).
    Escape,
}

/// What the app should do after an update.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    /// Save this task (the core's `save_todo`); report the answer with [`Detail::stored`] or
    /// [`Detail::save_failed`].
    Save(Todo),
    Delete(Todo),
    /// Close the detail; the app saves [`Detail::leave`] first.
    Close,
    /// Tick or untick the task with this uid (a subtask), as the list does.
    ToggleDone(String),
    /// Show the task with this uid in the detail instead.
    Show(String),
    /// Give the keyboard focus to the widget with this id.
    Focus(&'static str),
}

/// The priorities the menu offers. iCalendar has nine levels; the app uses these four.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    None,
    Low,
    Medium,
    High,
}

const PRIORITIES: [Priority; 4] = [Priority::None, Priority::High, Priority::Medium, Priority::Low];

impl Priority {
    /// The iCalendar priority stored for this choice; `None` means undefined.
    fn to_todo(self) -> Option<u8> {
        match self {
            Priority::None => None,
            Priority::Low => Some(9),
            Priority::Medium => Some(5),
            Priority::High => Some(1),
        }
    }

    /// The choice nearest to a stored priority: 1–4 High, 5 Medium, 6–9 Low. Undefined (0 or
    /// missing) and values outside 1–9 are None, the same as the task list treats them.
    fn from_todo(priority: Option<u8>) -> Priority {
        match priority {
            Some(1..=4) => Priority::High,
            Some(5) => Priority::Medium,
            Some(6..=9) => Priority::Low,
            _ => Priority::None,
        }
    }

    /// The short name, for chips and the menu.
    fn label(self) -> &'static str {
        match self {
            Priority::None => tr("No priority"),
            Priority::Low => tr("Low"),
            Priority::Medium => tr("Medium"),
            Priority::High => tr("High"),
        }
    }

    /// The long name, for the properties column ("Medium priority").
    fn long_label(self) -> &'static str {
        match self {
            Priority::None => tr("No priority"),
            Priority::Low => tr("Low priority"),
            Priority::Medium => tr("Medium priority"),
            Priority::High => tr("High priority"),
        }
    }

    /// How many of the three bars are lit.
    fn level(self) -> usize {
        list::priority_level(self.to_todo())
    }
}

/// The editable fields, as they will be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Draft {
    summary: String,
    completed: bool,
    /// [`Collection::id`] the task is saved into.
    collection: String,
    due: Option<EventTime>,
    priority: Priority,
    /// UID of the parent task.
    parent: Option<String>,
    notes: String,
}

impl Detail {
    /// The detail of `todo`; `lists` are the writable task lists, `all` every loaded task.
    pub fn new(todo: &Todo, lists: &[Collection], all: &[Todo]) -> Detail {
        let draft = draft_of(todo);
        Detail {
            original: todo.clone(),
            lists: lists.to_vec(),
            others: others_of(todo, all),
            today: Local::now().date_naive(),
            title: text_editor::Content::with_text(&draft.summary),
            notes: text_editor::Content::with_text(&draft.notes),
            due_text: due_input(draft.due),
            draft,
            open: None,
            confirm_delete: false,
            saving: false,
            sent: None,
            attempted: false,
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::TitleEdited(action) => {
                self.title.perform(action);
                // A title is one line; pasted line breaks become spaces.
                self.draft.summary = self.title.text().replace(['\r', '\n'], " ");
            }
            Message::TitleSubmit => return self.request_save(true),
            Message::NotesEdited(action) => {
                self.notes.perform(action);
                self.draft.notes = self.notes.text();
            }
            Message::Flush => return self.request_save(false),
            Message::ToggleDone => {
                self.draft.completed = !self.draft.completed;
                return self.request_save(true);
            }
            Message::Open(editor) => {
                self.confirm_delete = false;
                if self.open == Some(editor) {
                    self.open = None;
                } else {
                    self.open = Some(editor);
                    if editor == Editor::Due {
                        return Action::Focus(DUE_INPUT);
                    }
                }
            }
            Message::CloseEditor => self.open = None,
            Message::PrioritySelected(priority) => {
                self.open = None;
                self.draft.priority = priority;
                return self.request_save(true);
            }
            Message::ListSelected(id) => {
                self.open = None;
                // A parent in another list cannot stay: the server keeps parents within a list.
                let parent_fits = self
                    .draft
                    .parent
                    .as_ref()
                    .is_none_or(|uid| self.others.iter().any(|t| &t.uid == uid && t.collection == id));
                if !parent_fits {
                    self.draft.parent = None;
                }
                self.draft.collection = id;
                return self.request_save(true);
            }
            Message::ParentSelected(parent) => {
                self.open = None;
                self.draft.parent = parent;
                return self.request_save(true);
            }
            Message::DueChanged(input) => {
                if input.trim().is_empty() {
                    self.draft.due = None;
                } else if let Some(day) = datetime::parse_date(&input, self.today) {
                    self.draft.due = Some(due_with_date(self.draft.due, day));
                }
                self.due_text = input;
            }
            Message::DueSubmit => {
                if due_text_ok(&self.due_text, self.today) {
                    self.open = None;
                }
                return self.request_save(true);
            }
            Message::DueToday => return self.pick_due(Some(self.today)),
            Message::DueTomorrow => return self.pick_due(Some(self.today + Days::new(1))),
            Message::DueNextWeek => return self.pick_due(Some(next_week(self.today))),
            Message::DueRemove => return self.pick_due(None),
            Message::ToggleSubtask(uid) => return Action::ToggleDone(uid),
            Message::OpenSubtask(uid) => return Action::Show(uid),
            Message::Delete => {
                self.open = None;
                self.confirm_delete = true;
            }
            Message::CancelDelete => self.confirm_delete = false,
            Message::ConfirmDelete => {
                self.confirm_delete = false;
                return Action::Delete(self.original.clone());
            }
            Message::Close => return Action::Close,
            Message::Escape => {
                if self.open.is_some() {
                    self.open = None;
                } else if self.confirm_delete {
                    self.confirm_delete = false;
                } else {
                    return Action::Close;
                }
            }
        }
        Action::None
    }

    /// The task to save when the detail is closed or replaced: the draft if it changed, can be
    /// saved and no save is under way. An empty title or unreadable date is dropped, as Linear
    /// drops an empty title.
    pub fn leave(&self) -> Option<Todo> {
        (!self.saving && self.problem().is_none() && self.changed()).then(|| self.next())
    }

    /// Whether the app should send [`Message::Flush`] after a short pause: title or notes
    /// changed, they can be saved and no save is under way. The app subscribes to a timer only
    /// while this holds, so an idle detail costs nothing.
    pub fn wants_autosave(&self) -> bool {
        !self.saving && self.problem().is_none() && self.changed()
    }

    /// The app's answer to [`Action::Save`] (or any other save of this task, e.g. a tick in the
    /// list): `saved` is the stored version. Our own save keeps the draft (the user may have typed
    /// on) and sends what changed meanwhile; a save from elsewhere replaces the draft.
    pub fn stored(&mut self, saved: &Todo) -> Action {
        if !self.saving {
            self.reset(saved);
            return Action::None;
        }
        self.saving = false;
        self.original = saved.clone();
        self.request_save(false)
    }

    /// The app's answer to a failed [`Action::Save`]: the draft stays, the next change tries again.
    pub fn save_failed(&mut self) {
        self.saving = false;
    }

    /// After a reload: the new lists and tasks. When nothing is pending, a version of this task
    /// changed elsewhere replaces the draft.
    pub fn refresh(&mut self, lists: &[Collection], all: &[Todo]) {
        self.lists = lists.to_vec();
        self.others = others_of(&self.original, all);
        let loaded = all.iter().find(|t| t.uid == self.original.uid);
        if let Some(loaded) = loaded
            && !self.saving
            && !self.changed()
            && loaded != &self.original
        {
            self.reset(loaded);
        }
    }

    /// Starts a version from `todo`, dropping the draft.
    fn reset(&mut self, todo: &Todo) {
        let draft = draft_of(todo);
        if draft != self.draft {
            self.title = text_editor::Content::with_text(&draft.summary);
            self.notes = text_editor::Content::with_text(&draft.notes);
            self.due_text = due_input(draft.due);
            self.draft = draft;
        }
        self.original = todo.clone();
        self.sent = None;
    }

    /// The task as it would be saved now.
    fn next(&self) -> Todo {
        build_todo(&self.original, &self.draft)
    }

    /// Whether the draft differs from the stored task and from what was last sent.
    fn changed(&self) -> bool {
        let next = self.next();
        next != self.original && self.sent.as_ref() != Some(&next)
    }

    /// A save if the draft can and needs to be saved. `explicit` is a request by the user
    /// (Enter, a property change): from then on problems are shown.
    fn request_save(&mut self, explicit: bool) -> Action {
        if self.problem().is_some() {
            self.attempted |= explicit;
            return Action::None;
        }
        if self.saving || !self.changed() {
            return Action::None;
        }
        let next = self.next();
        self.saving = true;
        self.sent = Some(next.clone());
        Action::Save(next)
    }

    fn pick_due(&mut self, day: Option<NaiveDate>) -> Action {
        self.open = None;
        self.draft.due = day.map(|day| due_with_date(self.draft.due, day));
        self.due_text = due_input(self.draft.due);
        self.request_save(true)
    }

    /// Why the draft cannot be saved yet; `None` when it can.
    fn problem(&self) -> Option<&'static str> {
        self.title_problem().or_else(|| self.due_problem())
    }

    fn title_problem(&self) -> Option<&'static str> {
        self.draft.summary.trim().is_empty().then(|| tr("Enter a title."))
    }

    fn due_problem(&self) -> Option<&'static str> {
        (!due_text_ok(&self.due_text, self.today))
            .then(|| tr("Date not understood. Try 11.10.2026, today or tomorrow."))
    }

    /// A problem, once the user asked for a save.
    fn shown(&self, problem: Option<&'static str>) -> Option<&'static str> {
        problem.filter(|_| self.attempted)
    }

    // -----------------------------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------------------------

    /// The task as a side panel (`torro_gui::panel`): path "List › Filter" (`filter` is the list's
    /// current filter name, `None` when it is the task's own list), the title field, the notes,
    /// the properties as rows and the subtasks; "Delete task" in the footer. `anchor` is
    /// registered by the trigger of the open editor; pass the same one to [`Detail::layer`].
    /// `wrap` turns this module's messages into the app's, because the panel is built for one
    /// message type.
    pub fn panel<'a, M: Clone + 'a>(
        &'a self,
        filter: Option<String>,
        anchor: &Anchor,
        wrap: fn(Message) -> M,
    ) -> Panel<'a, M> {
        let mark = Mark::Ring(self.list_colour());
        let parts = std::iter::once(self.list_name()).chain(filter);
        let mut title = column![self.title_field()].spacing(space::S);
        if let Some(problem) = self.shown(self.title_problem()) {
            title = title.push(error_text(problem));
        }
        let mut body = column![panel::inset(self.notes_field()), self.properties(anchor)].spacing(space::L);
        if let Some(subtasks) = self.subtasks_view() {
            body = body.push(subtasks);
        }
        let footer = self.footer().into_iter().map(|element| element.map(wrap)).collect();
        panel::panel(
            panel::path(Some(mark), parts),
            Element::from(title).map(wrap),
            Element::from(body).map(wrap),
            wrap(Message::Close),
        )
        .footer(footer)
    }

    /// "Properties" (status, priority, due), "List" and "Parent task" as `prop_section`s: the same
    /// rows in every size.
    fn properties<'a>(&'a self, anchor: &Anchor) -> Element<'a, Message> {
        let priority = self.draft.priority;
        let status = widgets::prop_row(
            None,
            row![self.check(), typo::text(typo::BODY, self.status_label())]
                .spacing(space::S)
                .align_y(Alignment::Center),
            Some(Message::ToggleDone),
        );
        let priority_row = self.trigger(
            Editor::Priority,
            anchor,
            widgets::prop_row(
                None,
                row![bars_cell(priority.level()), typo::text(typo::BODY, priority.long_label())]
                    .spacing(space::S)
                    .align_y(Alignment::Center),
                Some(Message::Open(Editor::Priority)),
            ),
        );
        let due = self.trigger(
            Editor::Due,
            anchor,
            widgets::prop_row(Some(Mark::Icon(Icon::Calendar)), self.due_value(), Some(Message::Open(Editor::Due))),
        );
        let list_row = self.trigger(
            Editor::List,
            anchor,
            widgets::prop_row(
                Some(Mark::Ring(self.list_colour())),
                typo::text(typo::BODY, self.list_name()),
                self.can_move().then_some(Message::Open(Editor::List)),
            ),
        );
        let parent = self.trigger(
            Editor::Parent,
            anchor,
            widgets::prop_row(
                Some(Mark::Icon(Icon::ListTodo)),
                self.parent_value(),
                Some(Message::Open(Editor::Parent)),
            ),
        );
        column![
            widgets::prop_section(tr("Properties"), vec![status, priority_row, due]),
            widgets::prop_section(tr("List"), vec![list_row]),
            widgets::prop_section(tr("Parent task"), vec![parent]),
        ]
        .spacing(space::L)
        .into()
    }

    /// The open editor as a floating layer next to `anchor`, or `None`. Hand it to the shell's
    /// overlay; a click beside it sends [`Message::CloseEditor`].
    pub fn layer<'a>(&'a self, anchor: &Anchor) -> Option<Element<'a, Message>> {
        let card = match self.open? {
            Editor::Priority => self.priority_menu(),
            Editor::List => self.list_menu(),
            Editor::Parent => self.parent_menu(),
            Editor::Due => self.due_card(),
        };
        Some(widgets::floating(anchor, Placement::Below(Align::Start), card, Message::CloseEditor))
    }

    /// The title as a large borderless field that wraps; Enter saves, Escape leaves the field.
    fn title_field(&self) -> Element<'_, Message> {
        text_editor(&self.title)
            .placeholder(tr("Task title"))
            .on_action(Message::TitleEdited)
            .key_binding(|press| match press.key.as_ref() {
                // Only while the field has focus: otherwise Enter and Escape belong to the window
                // (open a task, close the detail).
                _ if !is_focused(&press) => text_editor::Binding::from_key_press(press),
                keyboard::Key::Named(keyboard::key::Named::Enter) => Some(text_editor::Binding::Sequence(vec![
                    text_editor::Binding::Custom(Message::TitleSubmit),
                    text_editor::Binding::Unfocus,
                ])),
                keyboard::Key::Named(keyboard::key::Named::Escape) => Some(leave_field()),
                _ => text_editor::Binding::from_key_press(press),
            })
            .font(typo::HEADLINE.font)
            .size(typo::HEADLINE.size)
            .line_height(typo::HEADLINE.line_height)
            .padding(Padding::ZERO)
            .wrapping(Wrapping::WordOrGlyph)
            .style(|t, s| bare_editor(t, s, palette(t).text))
            .into()
    }

    /// The notes as a quiet field in `text_2`, without a frame.
    fn notes_field(&self) -> Element<'_, Message> {
        text_editor(&self.notes)
            .placeholder(tr("Add notes …"))
            .on_action(Message::NotesEdited)
            .key_binding(|press| match press.key.as_ref() {
                _ if !is_focused(&press) => text_editor::Binding::from_key_press(press),
                keyboard::Key::Named(keyboard::key::Named::Escape) => Some(leave_field()),
                _ => text_editor::Binding::from_key_press(press),
            })
            .font(typo::BODY.font)
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .padding(Padding::ZERO)
            .wrapping(Wrapping::WordOrGlyph)
            .style(|t, s| bare_editor(t, s, palette(t).text_2))
            .into()
    }

    /// `content` anchored when `editor` is the open one, so its layer floats next to it.
    fn trigger<'a>(&self, editor: Editor, anchor: &Anchor, content: Element<'a, Message>) -> Element<'a, Message> {
        if self.open == Some(editor) { widgets::anchored(anchor, content) } else { content }
    }

    /// The round check of the task itself, in the list's colour.
    fn check(&self) -> Element<'_, Message> {
        checkbox(self.draft.completed)
            .size(style::CHECK_SIZE)
            .style(style::task_check(self.list_colour()))
            .on_toggle(|_| Message::ToggleDone)
            .into()
    }

    fn status_label(&self) -> &'static str {
        if self.draft.completed { tr("Done") } else { tr("Open") }
    }

    fn due_label(&self) -> String {
        match self.draft.due {
            Some(due) => list::due_text(due, self.today),
            None => tr("No due date").to_string(),
        }
    }

    fn due_value(&self) -> Element<'_, Message> {
        let label = typo::text(typo::BODY.tabular(), self.due_label());
        if self.draft.due.is_some() { label.into() } else { label.style(theme::text_3).into() }
    }

    fn parent_value(&self) -> Element<'_, Message> {
        match self.parent_title() {
            Some(title) => {
                container(typo::text(typo::BODY, title).wrapping(Wrapping::None)).width(Fill).clip(true).into()
            }
            None => typo::text(typo::BODY, tr("No parent task")).style(theme::text_3).into(),
        }
    }

    /// The parent's title; "Unknown task" for a parent that is not loaded.
    fn parent_title(&self) -> Option<String> {
        let uid = self.draft.parent.as_ref()?;
        Some(
            self.others
                .iter()
                .find(|t| &t.uid == uid)
                .map_or_else(|| tr("Unknown task").to_string(), |t| t.summary.clone()),
        )
    }

    fn list(&self) -> Option<&Collection> {
        self.lists.iter().find(|c| c.id == self.draft.collection)
    }

    fn list_name(&self) -> String {
        self.list().map_or_else(|| tr("List").to_string(), |c| c.name.clone())
    }

    fn list_colour(&self) -> Color {
        widgets::collection_color(self.list().and_then(|c| c.color.as_deref()))
    }

    /// Moving needs another writable list.
    fn can_move(&self) -> bool {
        self.lists.len() > 1
    }

    /// "Sub-tasks 1/3" and one row per subtask; `None` without subtasks.
    fn subtasks_view(&self) -> Option<Element<'_, Message>> {
        let subtasks = self.subtasks();
        if subtasks.is_empty() {
            return None;
        }
        let done = subtasks.iter().filter(|t| t.completed).count();
        let progress = format!("{done}/{}", subtasks.len());
        let head = row![
            typo::text(typo::CAPTION, tr("Sub-tasks")).style(theme::text_3),
            typo::text(typo::META.tabular(), progress).style(theme::text_3)
        ]
        .spacing(space::S)
        .align_y(Alignment::Center);
        let colour = self.list_colour();
        let rows = subtasks.into_iter().map(|task| subtask_row(task, colour));
        // The title stands where the titles of the property sections stand (8 in from the rows).
        let head = container(head).padding(Padding { bottom: space::XS, ..style::ITEM_PADDING });
        Some(column![head].extend(rows).into())
    }

    /// The direct subtasks, open first, then by title.
    fn subtasks(&self) -> Vec<&Todo> {
        let mut subtasks: Vec<&Todo> =
            self.others.iter().filter(|t| t.parent.as_deref() == Some(self.original.uid.as_str())).collect();
        subtasks.sort_by_key(|t| (t.completed, t.summary.to_lowercase()));
        subtasks
    }

    /// The panel's footer: "Delete task" (danger); after the first press the question with Delete
    /// and Cancel. Escape cancels the question before it closes the panel.
    fn footer(&self) -> Vec<Element<'_, Message>> {
        if !self.confirm_delete {
            return vec![widgets::text_button(None, tr("Delete task"), style::button::danger, Some(Message::Delete))];
        }
        vec![
            typo::text(typo::BODY, tr("Really delete?")).style(theme::text_2).into(),
            widgets::text_button(None, tr("Delete"), style::button::danger, Some(Message::ConfirmDelete)),
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CancelDelete)),
        ]
    }

    fn priority_menu(&self) -> Element<'_, Message> {
        let items = PRIORITIES
            .into_iter()
            .map(|choice| MenuItem {
                mark: None,
                label: choice.label().into(),
                trail: Trail::None,
                current: choice == self.draft.priority,
                on_press: Message::PrioritySelected(choice),
            })
            .collect();
        widgets::menu(vec![MenuSection::titled(tr("Priority"), items)])
    }

    fn list_menu(&self) -> Element<'_, Message> {
        let items = self
            .lists
            .iter()
            .map(|list| {
                MenuItem::new(
                    Mark::Ring(widgets::collection_color(list.color.as_deref())),
                    list.name.clone(),
                    Message::ListSelected(list.id.clone()),
                )
                .current(list.id == self.draft.collection)
            })
            .collect();
        widgets::menu(vec![MenuSection::titled(tr("Move to list"), items)])
    }

    /// "No parent task" and the possible parents; long menus scroll.
    fn parent_menu(&self) -> Element<'_, Message> {
        let items = self
            .parent_choices()
            .into_iter()
            .map(|(uid, title)| MenuItem {
                mark: None,
                current: uid == self.draft.parent,
                label: title.into(),
                trail: Trail::None,
                on_press: Message::ParentSelected(uid),
            })
            .collect();
        let menu = widgets::menu(vec![MenuSection::titled(tr("Parent task"), items)]);
        container(
            scrollable(menu).direction(scrollable::Direction::Vertical(style::scrollbar())).style(style::scrollable),
        )
        .max_height(style::MENU_ITEM_HEIGHT * PARENT_MENU_ROWS)
        .into()
    }

    /// The due card: quick choices as chips, then a field for any date.
    fn due_card(&self) -> Element<'_, Message> {
        let quick = widgets::chip_row(vec![
            widgets::chip(None, tr("Today"), Some(Message::DueToday)),
            widgets::chip(None, tr("Tomorrow"), Some(Message::DueTomorrow)),
            widgets::chip(None, tr("Next week"), Some(Message::DueNextWeek)),
            widgets::chip(None, tr("No date"), Some(Message::DueRemove)),
        ]);
        let field = text_input(tr("Pick a date, e.g. 11.10.2026"), &self.due_text)
            .id(DUE_INPUT)
            .on_input(Message::DueChanged)
            .on_submit(Message::DueSubmit)
            .padding(style::INPUT_PADDING)
            .font(typo::BODY.font)
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .style(style::text_input);
        let mut card = column![widgets::section_title(tr("Due date")), quick, field].spacing(space::S);
        if let Some(problem) = self.shown(self.due_problem()) {
            card = card.push(error_text(problem));
        } else if let Some(EventTime::DateTime(at)) = self.draft.due {
            let time = datetime::format_time(datetime::to_local(at).time());
            card =
                card.push(typo::text(typo::META, trf("Time {time} is kept", &[("time", &time)])).style(theme::text_3));
        }
        // As wide as a floating card, so the four quick choices fit on one line.
        container(card).width(widgets::POPOVER_WIDTH).padding(space::M).style(style::container::menu).into()
    }

    /// "No parent task", then the other tasks of the draft's list by title, without this task
    /// and its subtasks (they would make a cycle). A parent the list does not offer stays as
    /// "Unknown task", so the saved task keeps it.
    fn parent_choices(&self) -> Vec<(Option<String>, String)> {
        let excluded = self.descendants();
        let mut tasks: Vec<&Todo> = self
            .others
            .iter()
            .filter(|t| t.collection == self.draft.collection && !excluded.contains(&t.uid))
            .collect();
        tasks.sort_by_key(|t| t.summary.to_lowercase());

        let mut choices = vec![(None, tr("No parent task").to_string())];
        choices.extend(tasks.into_iter().map(|t| (Some(t.uid.clone()), t.summary.clone())));
        let unknown = self
            .draft
            .parent
            .as_ref()
            .filter(|uid| !choices.iter().any(|(choice, _)| choice.as_deref() == Some(uid.as_str())));
        if let Some(uid) = unknown {
            choices.push((Some(uid.clone()), tr("Unknown task").to_string()));
        }
        choices
    }

    /// UIDs of the tasks below this one: its subtasks, their subtasks, and so on.
    fn descendants(&self) -> HashSet<String> {
        let mut found = HashSet::new();
        let mut stack = vec![self.original.uid.as_str()];
        while let Some(uid) = stack.pop() {
            for task in self.others.iter().filter(|t| t.parent.as_deref() == Some(uid)) {
                if found.insert(task.uid.clone()) {
                    stack.push(task.uid.as_str());
                }
            }
        }
        found
    }
}

/// One subtask: round check in the list colour, the title (struck through when done) as a button
/// that opens it; 32 high with a line below.
fn subtask_row(task: &Todo, colour: Color) -> Element<'_, Message> {
    let uid = task.uid.clone();
    let check = checkbox(task.completed)
        .size(style::CHECK_SIZE)
        .style(style::task_check(colour))
        .on_toggle(move |_| Message::ToggleSubtask(uid.clone()));
    let title = button(container(list::title_view(task)).width(Fill).clip(true).center_y(Fill))
        .width(Fill)
        .height(size::CONTROL - LINE)
        .padding(style::ITEM_PADDING)
        .style(style::button::list_row(false))
        .on_press(Message::OpenSubtask(task.uid.clone()));
    // The check stands on the axis of the property marks (16 from the panel's edge).
    let line = container(row![check, title].align_y(Alignment::Center)).padding(Padding::ZERO.left(space::S));
    column![line, rule::horizontal(LINE).style(style::rule)].into()
}

/// Escape in a text field: leave it and save what was typed.
/// Whether the editor that got `press` has the keyboard focus.
fn is_focused(press: &text_editor::KeyPress) -> bool {
    matches!(press.status, text_editor::Status::Focused { .. })
}

fn leave_field() -> text_editor::Binding<Message> {
    text_editor::Binding::Sequence(vec![text_editor::Binding::Unfocus, text_editor::Binding::Custom(Message::Flush)])
}

/// A text editor without frame or surface (Linear's title and description): text in `value`,
/// placeholder `text_3`. Focus shows as the caret; the field sits where its text is.
fn bare_editor(theme: &Theme, status: text_editor::Status, value: Color) -> text_editor::Style {
    let p = palette(theme);
    let base = style::text_editor(theme, status);
    let faded = matches!(status, text_editor::Status::Disabled);
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: border::rounded(0).width(0).color(Color::TRANSPARENT),
        placeholder: p.text_3,
        value: if faded { value.scale_alpha(style::DISABLED_ALPHA) } else { value },
        ..base
    }
}

/// The priority bars in a 16-wide cell, so they line up with the icons of the other rows.
fn bars_cell<'a>(level: usize) -> Element<'a, Message> {
    container(list::priority_view(level)).center_x(icon::SIZE).into()
}

fn error_text<'a>(problem: &'a str) -> Element<'a, Message> {
    typo::text(typo::META, problem)
        .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).error_text) })
        .into()
}

fn others_of(todo: &Todo, all: &[Todo]) -> Vec<Todo> {
    all.iter().filter(|t| t.uid != todo.uid).cloned().collect()
}

fn draft_of(todo: &Todo) -> Draft {
    Draft {
        summary: todo.summary.clone(),
        completed: todo.completed,
        collection: todo.collection.clone(),
        due: todo.due,
        priority: Priority::from_todo(todo.priority),
        parent: todo.parent.clone(),
        notes: todo.description.clone().unwrap_or_default(),
    }
}

/// The task as it is saved: the loaded task with the draft's fields. `uid`, `href` and `etag`
/// stay as loaded; the etag is what the server checks the write against. A stored priority that
/// maps to the unchanged choice is kept as it was.
fn build_todo(original: &Todo, draft: &Draft) -> Todo {
    let notes = draft.notes.trim_end();
    let priority = if Priority::from_todo(original.priority) == draft.priority {
        original.priority
    } else {
        draft.priority.to_todo()
    };
    Todo {
        uid: original.uid.clone(),
        // On the server a move between lists is a delete plus a create. The core does that
        // later; for now only the field changes.
        collection: draft.collection.clone(),
        href: original.href.clone(),
        etag: original.etag.clone(),
        summary: draft.summary.trim().to_string(),
        due: draft.due,
        priority,
        completed: draft.completed,
        parent: draft.parent.clone(),
        description: (!notes.is_empty()).then(|| notes.to_string()),
    }
}

/// The local day a due falls on.
fn due_day(due: EventTime) -> NaiveDate {
    match due {
        EventTime::Date(day) => day,
        EventTime::DateTime(at) => datetime::to_local(at).date(),
    }
}

/// What the date field shows for `due`.
fn due_input(due: Option<EventTime>) -> String {
    due.map(|due| datetime::format_date(due_day(due))).unwrap_or_default()
}

/// The due after the user picks `day`. A date-time due keeps its local time of day; a date
/// stays a date, and a new due is a whole day.
fn due_with_date(current: Option<EventTime>, day: NaiveDate) -> EventTime {
    match current {
        Some(EventTime::DateTime(at)) => {
            let time = datetime::to_local(at).time();
            EventTime::DateTime(datetime::to_utc(day.and_time(time)))
        }
        _ => EventTime::Date(day),
    }
}

/// "Next week": the first day of the next week (`datetime::week_start`), as in Things and Todoist.
fn next_week(today: NaiveDate) -> NaiveDate {
    datetime::week_start(today) + Days::new(7)
}

/// Empty is fine (no due); otherwise the text must be a date.
fn due_text_ok(text: &str, today: NaiveDate) -> bool {
    text.trim().is_empty() || datetime::parse_date(text, today).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use chrono::NaiveDateTime;
    use iced::widget::Space;
    use torro_gui::layout::SizeClass;
    use torro_pim::CollectionKind;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).expect("valid date")
    }

    fn at(d: u32, hour: u32, minute: u32) -> NaiveDateTime {
        day(d).and_hms_opt(hour, minute, 0).expect("valid time")
    }

    fn todo(uid: &str, summary: &str) -> Todo {
        Todo {
            uid: uid.into(),
            collection: "work".into(),
            href: None,
            etag: None,
            summary: summary.into(),
            due: None,
            priority: None,
            completed: false,
            parent: None,
            description: None,
        }
    }

    fn list(id: &str, name: &str) -> Collection {
        Collection { id: id.into(), name: name.into(), kind: CollectionKind::Todos, color: None, writable: true }
    }

    fn lists() -> Vec<Collection> {
        vec![list("work", "Work"), list("home", "Home")]
    }

    /// The panel for `task`, with `others` next to it and today fixed to Sunday 11 October 2026.
    fn detail(task: &Todo, others: &[Todo]) -> Detail {
        let mut all = others.to_vec();
        all.push(task.clone());
        let mut detail = Detail::new(task, &lists(), &all);
        detail.today = day(11);
        detail
    }

    /// The task an update hands out to be saved, or a panic when it saves nothing.
    fn saves(detail: &mut Detail, message: Message) -> Todo {
        match detail.update(message) {
            Action::Save(todo) => todo,
            other => panic!("expected a save, got {other:?}"),
        }
    }

    /// The core's answer to a save: the same task with a new etag.
    fn store(detail: &mut Detail, mut todo: Todo, etag: &str) -> Action {
        todo.etag = Some(etag.into());
        detail.stored(&todo)
    }

    fn type_title(detail: &mut Detail, title: &str) {
        detail.update(Message::TitleEdited(text_editor::Action::SelectAll));
        let paste = text_editor::Edit::Paste(Arc::new(title.to_string()));
        detail.update(Message::TitleEdited(text_editor::Action::Edit(paste)));
    }

    fn type_notes(detail: &mut Detail, notes: &str) {
        detail.update(Message::NotesEdited(text_editor::Action::SelectAll));
        let paste = text_editor::Edit::Paste(Arc::new(notes.to_string()));
        detail.update(Message::NotesEdited(text_editor::Action::Edit(paste)));
    }

    #[test]
    fn saved_task_keeps_identity_and_trims_title() {
        let mut task = todo("t1", "Buy milk");
        task.href = Some("/cal/work/t1.ics".into());
        task.etag = Some("\"v7\"".into());
        let mut detail = detail(&task, &[]);
        type_title(&mut detail, "  Buy oat milk  ");
        let todo = saves(&mut detail, Message::TitleSubmit);
        assert_eq!(todo.uid, "t1");
        assert_eq!(todo.href.as_deref(), Some("/cal/work/t1.ics"));
        assert_eq!(todo.etag.as_deref(), Some("\"v7\""));
        assert_eq!(todo.collection, "work");
        assert_eq!(todo.summary, "Buy oat milk");
    }

    #[test]
    fn pasted_line_breaks_stay_out_of_the_title() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        type_title(&mut detail, "two\nlines");
        assert_eq!(saves(&mut detail, Message::TitleSubmit).summary, "two lines");
    }

    #[test]
    fn notes_map_to_description() {
        let mut draft = draft_of(&todo("t1", "a"));
        draft.notes = "  \n\n".into();
        assert_eq!(build_todo(&todo("t1", "a"), &draft).description, None);
        draft.notes = "Two lines\nlike this\n\n".into();
        assert_eq!(build_todo(&todo("t1", "a"), &draft).description.as_deref(), Some("Two lines\nlike this"));
    }

    #[test]
    fn priority_maps_stored_values_to_choices() {
        assert_eq!(Priority::from_todo(None), Priority::None);
        assert_eq!(Priority::from_todo(Some(0)), Priority::None);
        assert_eq!(Priority::from_todo(Some(1)), Priority::High);
        assert_eq!(Priority::from_todo(Some(4)), Priority::High);
        assert_eq!(Priority::from_todo(Some(5)), Priority::Medium);
        assert_eq!(Priority::from_todo(Some(6)), Priority::Low);
        assert_eq!(Priority::from_todo(Some(9)), Priority::Low);
        assert_eq!(Priority::from_todo(Some(10)), Priority::None);
    }

    #[test]
    fn priority_maps_choices_to_stored_values() {
        assert_eq!(Priority::None.to_todo(), None);
        assert_eq!(Priority::Low.to_todo(), Some(9));
        assert_eq!(Priority::Medium.to_todo(), Some(5));
        assert_eq!(Priority::High.to_todo(), Some(1));
        for choice in PRIORITIES {
            assert_eq!(Priority::from_todo(choice.to_todo()), choice);
        }
    }

    #[test]
    fn chosen_priority_is_saved_at_once() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        detail.update(Message::Open(Editor::Priority));
        assert_eq!(saves(&mut detail, Message::PrioritySelected(Priority::Low)).priority, Some(9));
        assert_eq!(detail.open, None, "picking closes the menu");
    }

    #[test]
    fn an_unchanged_odd_priority_is_kept() {
        let task = Todo { priority: Some(3), ..todo("t1", "a") };
        let mut detail = detail(&task, &[]);
        type_title(&mut detail, "b");
        assert_eq!(saves(&mut detail, Message::TitleSubmit).priority, Some(3));
    }

    #[test]
    fn typed_date_becomes_a_whole_day() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        detail.update(Message::DueChanged("12.10.2026".into()));
        assert_eq!(saves(&mut detail, Message::DueSubmit).due, Some(EventTime::Date(day(12))));
    }

    #[test]
    fn date_time_due_keeps_its_time_when_the_date_changes() {
        let mut task = todo("t1", "a");
        task.due = Some(EventTime::DateTime(datetime::to_utc(at(11, 14, 30))));
        let mut detail = detail(&task, &[]);
        detail.update(Message::DueChanged("12.10.2026".into()));
        match saves(&mut detail, Message::DueSubmit).due {
            Some(EventTime::DateTime(instant)) => assert_eq!(datetime::to_local(instant), at(12, 14, 30)),
            other => panic!("expected a date-time due, got {other:?}"),
        }
    }

    #[test]
    fn quick_choices_set_the_day() {
        let task = todo("t1", "a");
        let cases = [
            (Message::DueTomorrow, Some(EventTime::Date(day(12)))),
            (Message::DueNextWeek, Some(EventTime::Date(next_week(day(11))))),
            (Message::DueRemove, None),
        ];
        for (message, due) in cases {
            let mut detail = detail(&Todo { due: Some(EventTime::Date(day(11))), ..task.clone() }, &[]);
            assert_eq!(saves(&mut detail, message).due, due);
        }
        let mut detail = detail(&task, &[]);
        assert_eq!(saves(&mut detail, Message::DueToday).due, Some(EventTime::Date(day(11))));
        assert_eq!(detail.due_text, "11.10.2026");
    }

    #[test]
    fn next_week_is_the_start_of_the_next_week() {
        let next = next_week(day(11));
        assert!(next > day(11) && next <= day(18));
        assert_eq!(datetime::week_start(next), next);
    }

    #[test]
    fn quick_choices_keep_the_time_of_a_date_time_due() {
        let mut task = todo("t1", "a");
        task.due = Some(EventTime::DateTime(datetime::to_utc(at(11, 9, 5))));
        let mut detail = detail(&task, &[]);
        match saves(&mut detail, Message::DueTomorrow).due {
            Some(EventTime::DateTime(instant)) => assert_eq!(datetime::to_local(instant), at(12, 9, 5)),
            other => panic!("expected a date-time due, got {other:?}"),
        }
    }

    #[test]
    fn unreadable_date_blocks_saving_and_shows_only_after_enter() {
        let mut task = todo("t1", "a");
        task.due = Some(EventTime::Date(day(12)));
        let mut detail = detail(&task, &[]);
        detail.update(Message::DueChanged("31.02.".into()));
        assert_eq!(detail.shown(detail.due_problem()), None, "no hint while typing");
        assert!(!detail.wants_autosave());
        assert_eq!(detail.update(Message::DueSubmit), Action::None);
        assert!(detail.shown(detail.due_problem()).is_some(), "hint after Enter");
        assert_eq!(detail.draft.due, Some(EventTime::Date(day(12))));
    }

    #[test]
    fn empty_title_is_not_saved_and_hinted_after_enter() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        type_title(&mut detail, "   ");
        assert!(!detail.wants_autosave());
        assert_eq!(detail.shown(detail.title_problem()), None);
        assert_eq!(detail.update(Message::TitleSubmit), Action::None);
        assert_eq!(detail.shown(detail.title_problem()), Some(tr("Enter a title.")));
        assert_eq!(detail.leave(), None, "leaving drops an empty title");
    }

    #[test]
    fn typing_waits_for_the_autosave_tick() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert!(!detail.wants_autosave());
        type_notes(&mut detail, "more");
        assert!(detail.wants_autosave());
        let todo = saves(&mut detail, Message::Flush);
        assert_eq!(todo.description.as_deref(), Some("more"));
        assert!(!detail.wants_autosave(), "nothing more while the save is under way");
    }

    #[test]
    fn one_save_at_a_time_and_the_next_carries_the_new_etag() {
        let mut detail = detail(&Todo { etag: Some("v1".into()), ..todo("t1", "a") }, &[]);
        let first = saves(&mut detail, Message::ToggleDone);
        assert!(first.completed);
        // Typed while the first save is under way: waits.
        type_title(&mut detail, "b");
        assert_eq!(detail.update(Message::PrioritySelected(Priority::High)), Action::None);
        match store(&mut detail, first, "v2") {
            Action::Save(next) => {
                assert_eq!(next.etag.as_deref(), Some("v2"));
                assert_eq!(next.summary, "b");
                assert_eq!(next.priority, Some(1));
                assert!(next.completed);
                assert_eq!(store(&mut detail, next, "v3"), Action::None, "nothing left to save");
            }
            other => panic!("expected the follow-up save, got {other:?}"),
        }
        assert_eq!(detail.title.text(), "b", "the typed title stays in the field");
    }

    #[test]
    fn a_save_from_elsewhere_replaces_the_draft() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        let ticked = Todo { completed: true, etag: Some("v2".into()), ..todo("t1", "a") };
        assert_eq!(detail.stored(&ticked), Action::None);
        assert!(detail.draft.completed);
        assert_eq!(detail.original.etag.as_deref(), Some("v2"));
    }

    #[test]
    fn a_reload_takes_changes_only_when_nothing_is_pending() {
        let task = todo("t1", "a");
        let mut detail = detail(&task, &[]);
        let renamed = Todo { summary: "renamed".into(), ..task.clone() };
        detail.refresh(&lists(), std::slice::from_ref(&renamed));
        assert_eq!(detail.draft.summary, "renamed");

        type_notes(&mut detail, "mine");
        let other = Todo { summary: "other".into(), ..task };
        detail.refresh(&lists(), &[other]);
        assert_eq!(detail.draft.summary, "renamed", "typed notes are not thrown away");
    }

    #[test]
    fn a_failed_save_lets_the_next_change_try_again() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        saves(&mut detail, Message::ToggleDone);
        detail.save_failed();
        assert!(saves(&mut detail, Message::PrioritySelected(Priority::High)).completed);
    }

    #[test]
    fn leaving_saves_what_is_pending() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert_eq!(detail.leave(), None);
        type_title(&mut detail, "b");
        assert_eq!(detail.leave().map(|t| t.summary), Some("b".into()));
    }

    #[test]
    fn parent_choices_exclude_self_and_own_descendants() {
        let task = todo("t1", "parent");
        let others = vec![
            Todo { parent: Some("t1".into()), ..todo("child", "child") },
            Todo { parent: Some("child".into()), ..todo("grandchild", "grandchild") },
            todo("other", "Other"),
            Todo { collection: "home".into(), ..todo("elsewhere", "elsewhere") },
        ];
        let detail = detail(&task, &others);
        let uids: Vec<Option<String>> = detail.parent_choices().into_iter().map(|(uid, _)| uid).collect();
        assert_eq!(uids, [None, Some("other".into())]);
    }

    #[test]
    fn parent_choices_follow_the_draft_list() {
        let task = todo("t1", "a");
        let others = vec![todo("other", "Other"), Todo { collection: "home".into(), ..todo("elsewhere", "B") }];
        let mut detail = detail(&task, &others);
        detail.update(Message::ListSelected("home".into()));
        let uids: Vec<Option<String>> = detail.parent_choices().into_iter().map(|(uid, _)| uid).collect();
        assert_eq!(uids, [None, Some("elsewhere".into())]);
    }

    #[test]
    fn parent_from_another_list_is_dropped_when_the_list_changes() {
        let mut task = todo("t1", "a");
        task.parent = Some("other".into());
        let mut detail = detail(&task, &[todo("other", "Other")]);
        let moved = saves(&mut detail, Message::ListSelected("home".into()));
        assert_eq!(moved.parent, None);
        assert_eq!(moved.collection, "home");
    }

    #[test]
    fn unknown_parent_is_kept_and_listed() {
        let mut task = todo("t1", "a");
        task.parent = Some("gone".into());
        let detail = detail(&task, &[]);
        assert_eq!(detail.parent_choices().last().and_then(|(uid, _)| uid.as_deref()), Some("gone"));
        assert_eq!(detail.parent_title(), Some(tr("Unknown task").to_string()));
    }

    #[test]
    fn chosen_parent_is_saved() {
        let mut detail = detail(&todo("t1", "a"), &[todo("p", "Parent")]);
        assert_eq!(saves(&mut detail, Message::ParentSelected(Some("p".into()))).parent.as_deref(), Some("p"));
    }

    #[test]
    fn subtasks_are_the_direct_children_open_first() {
        let others = vec![
            Todo { parent: Some("t1".into()), completed: true, ..todo("c1", "Alpha") },
            Todo { parent: Some("t1".into()), ..todo("c2", "Zulu") },
            Todo { parent: Some("c2".into()), ..todo("g", "Grandchild") },
            todo("x", "Unrelated"),
        ];
        let detail = detail(&todo("t1", "a"), &others);
        let uids: Vec<&str> = detail.subtasks().iter().map(|t| t.uid.as_str()).collect();
        assert_eq!(uids, ["c2", "c1"]);
    }

    #[test]
    fn subtask_clicks_go_to_the_app() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert_eq!(detail.update(Message::ToggleSubtask("c".into())), Action::ToggleDone("c".into()));
        assert_eq!(detail.update(Message::OpenSubtask("c".into())), Action::Show("c".into()));
    }

    #[test]
    fn delete_asks_once_before_deleting() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert_eq!(detail.update(Message::Delete), Action::None);
        assert!(detail.confirm_delete);
        assert_eq!(detail.update(Message::CancelDelete), Action::None);
        assert!(!detail.confirm_delete);

        detail.update(Message::Delete);
        match detail.update(Message::ConfirmDelete) {
            Action::Delete(todo) => assert_eq!(todo.uid, "t1"),
            other => panic!("expected a delete, got {other:?}"),
        }
        assert!(!detail.confirm_delete);
    }

    #[test]
    fn escape_closes_editor_then_question_then_detail() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert_eq!(detail.update(Message::Open(Editor::Due)), Action::Focus(DUE_INPUT));
        assert_eq!(detail.update(Message::Escape), Action::None);
        assert_eq!(detail.open, None);
        detail.update(Message::Delete);
        assert_eq!(detail.update(Message::Escape), Action::None);
        assert!(!detail.confirm_delete);
        assert_eq!(detail.update(Message::Escape), Action::Close);
    }

    #[test]
    fn a_second_click_closes_the_editor() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        detail.update(Message::Open(Editor::List));
        assert_eq!(detail.open, Some(Editor::List));
        detail.update(Message::Open(Editor::List));
        assert_eq!(detail.open, None);
    }

    #[test]
    fn the_footer_asks_before_deleting() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert_eq!(detail.footer().len(), 1, "only Delete task");
        detail.update(Message::Delete);
        assert_eq!(detail.footer().len(), 3, "the question, Delete and Cancel");
        detail.update(Message::CancelDelete);
        assert_eq!(detail.footer().len(), 1);
    }

    #[test]
    fn the_panel_is_the_same_for_every_size_and_closes_with_its_x() {
        let child = Todo { parent: Some("t1".into()), ..todo("c", "Child") };
        let mut detail = detail(&todo("t1", "a"), &[child]);
        detail.update(Message::Open(Editor::List));
        let anchor = Anchor::new();
        for class in [SizeClass::Mini, SizeClass::Strip, SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let panel = detail.panel(Some("Today".into()), &anchor, |message| message);
            let placed = panel::place(class, Space::new(), Some(panel), detail.layer(&anchor));
            assert!(placed.overlay.is_some(), "{class:?}: the open editor is always on top");
        }
    }

    #[test]
    fn close_closes() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert_eq!(detail.update(Message::Close), Action::Close);
    }
}
