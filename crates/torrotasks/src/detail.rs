//! The detail panel for one task (task K.15 in `docs/tasks/kalender-aufgaben.md`).
//!
//! The fields are a draft that starts from the loaded task:
//! - Title: text field; Enter saves.
//! - Done: checkbox.
//! - List: pick list of the task lists. Moving a task changes its collection; a parent in
//!   another list is dropped.
//! - Due: a date typed as `11.10.2026`, `today` or `tomorrow` (`datetime::parse_date`), or set
//!   with the Today, Tomorrow and Remove buttons. A date-time due keeps its time when only the
//!   date changes; a new due is a whole day.
//! - Priority: None, Low (9), Medium (5), High (1). Stored values map to the nearest choice.
//! - Parent task: pick list of the other tasks in the same list, minus the task's own subtasks.
//! - Notes: multi-line text.
//!
//! Save hands the changed task to the app as [`Action::Save`]. Delete asks once ("Really
//! delete?") before [`Action::Delete`]. Close closes the panel. Input that cannot be read, or
//! an empty title, shows a hint and blocks saving.

use std::collections::HashSet;
use std::fmt;

use chrono::{Local, NaiveDate};
use iced::widget::{
    Space, button, checkbox, column, container, pick_list, row, scrollable, text, text_editor, text_input,
};
use iced::{Background, Element, Length};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime;
use torro_gui::theme;
use torro_gui::widgets::section_title;
use torro_pim::{Collection, EventTime, Todo};

/// The panel's state.
#[derive(Debug)]
pub struct Detail {
    /// The task as it was loaded; the base for the saved version (keeps uid, href and etag).
    pub original: Todo,
    pub lists: Vec<Collection>,
    /// Other tasks, for choosing a parent.
    pub others: Vec<Todo>,
    /// Today, for the due date keywords and the Today and Tomorrow buttons.
    today: NaiveDate,
    draft: Draft,
    /// The due date as typed. It can be unfinished or invalid; then the draft keeps its last
    /// valid due and saving is blocked.
    due_text: String,
    /// The notes editor. `draft.notes` mirrors its text.
    notes: text_editor::Content,
    /// Set by the first press on Delete; the second press confirms.
    confirm_delete: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    TitleChanged(String),
    DoneToggled(bool),
    ListSelected(ListChoice),
    DueChanged(String),
    DueToday,
    DueTomorrow,
    DueRemove,
    PrioritySelected(Priority),
    ParentSelected(ParentChoice),
    NotesEdited(text_editor::Action),
    Save,
    Delete,
    CancelDelete,
    ConfirmDelete,
    Close,
}

/// What the app should do after an update.
#[derive(Debug, Clone)]
pub enum Action {
    None,
    Save(Todo),
    Delete(Todo),
    Close,
}

/// The priorities the pick list offers. iCalendar has nine levels; the app uses these four.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    None,
    Low,
    Medium,
    High,
}

const PRIORITIES: [Priority; 4] = [Priority::None, Priority::Low, Priority::Medium, Priority::High];

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
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Priority::None => tr("None"),
            Priority::Low => tr("Low"),
            Priority::Medium => tr("Medium"),
            Priority::High => tr("High"),
        })
    }
}

/// An entry of the list pick list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListChoice {
    id: String,
    name: String,
}

impl fmt::Display for ListChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// An entry of the parent pick list. `uid` is `None` for "No parent task".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentChoice {
    uid: Option<String>,
    title: String,
}

impl fmt::Display for ParentChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.title)
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
    pub fn new(todo: &Todo, lists: &[Collection], all: &[Todo]) -> Detail {
        let draft = draft_of(todo);
        Detail {
            original: todo.clone(),
            lists: lists.to_vec(),
            others: all.iter().filter(|t| t.uid != todo.uid).cloned().collect(),
            today: Local::now().date_naive(),
            due_text: todo.due.map(|due| datetime::format_date(due_day(due))).unwrap_or_default(),
            notes: text_editor::Content::with_text(&draft.notes),
            draft,
            confirm_delete: false,
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::TitleChanged(summary) => self.draft.summary = summary,
            Message::DoneToggled(completed) => self.draft.completed = completed,
            Message::ListSelected(list) => {
                // A parent in another list cannot stay: the server keeps parents within a list.
                let parent_fits = self
                    .draft
                    .parent
                    .as_ref()
                    .is_none_or(|uid| self.others.iter().any(|t| &t.uid == uid && t.collection == list.id));
                if !parent_fits {
                    self.draft.parent = None;
                }
                self.draft.collection = list.id;
            }
            Message::DueChanged(input) => {
                if input.trim().is_empty() {
                    self.draft.due = None;
                } else if let Some(day) = datetime::parse_date(&input, self.today) {
                    self.draft.due = Some(due_with_date(self.draft.due, day));
                }
                self.due_text = input;
            }
            Message::DueToday => self.set_due_day(self.today),
            Message::DueTomorrow => self.set_due_day(self.today.succ_opt().unwrap_or(self.today)),
            Message::DueRemove => {
                self.draft.due = None;
                self.due_text.clear();
            }
            Message::PrioritySelected(priority) => self.draft.priority = priority,
            Message::ParentSelected(parent) => self.draft.parent = parent.uid,
            Message::NotesEdited(action) => {
                self.notes.perform(action);
                self.draft.notes = self.notes.text();
            }
            Message::Save => {
                if self.problem().is_some() {
                    return Action::None;
                }
                return Action::Save(build_todo(&self.original, &self.draft));
            }
            Message::Delete => self.confirm_delete = true,
            Message::CancelDelete => self.confirm_delete = false,
            Message::ConfirmDelete => {
                self.confirm_delete = false;
                return Action::Delete(self.original.clone());
            }
            Message::Close => return Action::Close,
        }
        Action::None
    }

    pub fn view(&self) -> Element<'_, Message> {
        let title = text_input(tr("Title"), &self.draft.summary)
            .on_input(Message::TitleChanged)
            .on_submit(Message::Save)
            .size(18)
            .padding(8);
        let done = checkbox(self.draft.completed).label(tr("Done")).on_toggle(Message::DoneToggled);
        let lists = self.list_choices();
        let selected_list = lists.iter().find(|list| list.id == self.draft.collection).cloned();
        let list = pick_list(lists, selected_list, Message::ListSelected).width(Length::Fill);

        let due = text_input(tr("Date, e.g. 11.10.2026 or tomorrow"), &self.due_text)
            .on_input(Message::DueChanged)
            .padding(8);
        let due_buttons = row![
            button(text(tr("Today"))).style(button::secondary).on_press(Message::DueToday),
            button(text(tr("Tomorrow"))).style(button::secondary).on_press(Message::DueTomorrow),
            button(text(tr("Remove"))).style(button::secondary).on_press(Message::DueRemove),
        ]
        .spacing(6);

        let priority = pick_list(PRIORITIES, Some(self.draft.priority), Message::PrioritySelected).width(Length::Fill);

        let parents = self.parent_choices();
        let selected_parent = parents.iter().find(|parent| parent.uid == self.draft.parent).cloned();
        let parent = pick_list(parents, selected_parent, Message::ParentSelected).width(Length::Fill);

        let notes = text_editor(&self.notes).on_action(Message::NotesEdited).height(Length::Fixed(140.0)).padding(8);

        let mut body = column![
            section_title(tr("Title")),
            title,
            done,
            section_title(tr("List")),
            list,
            section_title(tr("Due")),
            due,
            due_buttons,
        ]
        .spacing(8);
        if let Some(due) = self.draft.due {
            body = body.push(self.due_hint(due));
        }
        body = body.push(column![section_title(tr("Priority")), priority].spacing(8));
        body = body.push(column![section_title(tr("Parent task")), parent].spacing(8));
        body = body.push(column![section_title(tr("Notes")), notes].spacing(8));
        if let Some(problem) = self.problem() {
            body = body.push(text(problem).size(13).color(theme::attention()));
        }
        body = body.push(self.actions());

        let panel = scrollable(body.padding(16).width(Length::Fill)).width(Length::Fill).height(Length::Fill);
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style { background: Some(Background::Color(theme::SURFACE)), ..Default::default() })
            .into()
    }

    /// Why the draft cannot be saved yet; `None` when it can.
    fn problem(&self) -> Option<&'static str> {
        if self.draft.summary.trim().is_empty() {
            Some(tr("Enter a title."))
        } else if !due_text_ok(&self.due_text, self.today) {
            Some(tr("Date not understood. Try 11.10.2026, today or tomorrow."))
        } else {
            None
        }
    }

    fn set_due_day(&mut self, day: NaiveDate) {
        self.draft.due = Some(due_with_date(self.draft.due, day));
        self.due_text = datetime::format_date(day);
    }

    /// Under the due field: the time a date-time due keeps when only the date changes.
    fn due_hint<'a>(&self, due: EventTime) -> Element<'a, Message> {
        match due {
            EventTime::Date(_) => Space::new().into(),
            EventTime::DateTime(at) => {
                let time = datetime::format_time(datetime::to_local(at).time());
                text(trf("Time {time} is kept", &[("time", &time)])).size(13).color(theme::MUTED).into()
            }
        }
    }

    /// Delete on the left; Close and Save on the right. After the first Delete press the row
    /// asks for confirmation instead.
    fn actions(&self) -> Element<'_, Message> {
        if self.confirm_delete {
            return column![
                text(tr("Really delete?")),
                row![
                    button(text(tr("Delete"))).style(button::danger).on_press(Message::ConfirmDelete),
                    button(text(tr("Cancel"))).style(button::secondary).on_press(Message::CancelDelete),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .into();
        }
        let save = button(text(tr("Save")))
            .style(button::primary)
            .on_press_maybe(self.problem().is_none().then_some(Message::Save));
        row![
            button(text(tr("Delete"))).style(button::danger).on_press(Message::Delete),
            Space::new().width(Length::Fill),
            button(text(tr("Close"))).style(button::secondary).on_press(Message::Close),
            save,
        ]
        .spacing(8)
        .into()
    }

    fn list_choices(&self) -> Vec<ListChoice> {
        self.lists.iter().map(|list| ListChoice { id: list.id.clone(), name: list.name.clone() }).collect()
    }

    /// "No parent task", then the other tasks of the draft's list by title, without this task
    /// and its subtasks (they would make a cycle). A parent the list does not offer stays as
    /// "Unknown task", so the saved task keeps it.
    fn parent_choices(&self) -> Vec<ParentChoice> {
        let excluded = self.descendants();
        let mut tasks: Vec<&Todo> = self
            .others
            .iter()
            .filter(|t| t.collection == self.draft.collection && !excluded.contains(&t.uid))
            .collect();
        tasks.sort_by_key(|t| t.summary.to_lowercase());

        let mut choices = vec![ParentChoice { uid: None, title: tr("No parent task").to_string() }];
        choices.extend(tasks.into_iter().map(|t| ParentChoice { uid: Some(t.uid.clone()), title: t.summary.clone() }));
        let unknown = self
            .draft
            .parent
            .as_ref()
            .filter(|uid| !choices.iter().any(|choice| choice.uid.as_deref() == Some(uid.as_str())));
        if let Some(uid) = unknown {
            choices.push(ParentChoice { uid: Some(uid.clone()), title: tr("Unknown task").to_string() });
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
/// stay as loaded; the etag is what the server checks the write against.
fn build_todo(original: &Todo, draft: &Draft) -> Todo {
    let notes = draft.notes.trim_end();
    Todo {
        uid: original.uid.clone(),
        // On the server a move between lists is a delete plus a create. The core does that
        // later; for now only the field changes.
        collection: draft.collection.clone(),
        href: original.href.clone(),
        etag: original.etag.clone(),
        summary: draft.summary.trim().to_string(),
        due: draft.due,
        priority: draft.priority.to_todo(),
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

/// Empty is fine (no due); otherwise the text must be a date.
fn due_text_ok(text: &str, today: NaiveDate) -> bool {
    text.trim().is_empty() || datetime::parse_date(text, today).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
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

    /// The panel for `task`, with `others` next to it and today fixed to 11 October 2026.
    fn detail(task: &Todo, others: &[Todo]) -> Detail {
        let mut all = others.to_vec();
        all.push(task.clone());
        let mut detail = Detail::new(task, &lists(), &all);
        detail.today = day(11);
        detail
    }

    /// The task the panel would save, or a panic when it does not save.
    fn saved(detail: &mut Detail) -> Todo {
        match detail.update(Message::Save) {
            Action::Save(todo) => todo,
            other => panic!("expected a save, got {other:?}"),
        }
    }

    fn set_due(detail: &mut Detail, input: &str) {
        detail.update(Message::DueChanged(input.into()));
    }

    #[test]
    fn saved_task_keeps_identity_and_trims_title() {
        let mut task = todo("t1", "Buy milk");
        task.href = Some("/cal/work/t1.ics".into());
        task.etag = Some("\"v7\"".into());
        let mut detail = detail(&task, &[]);
        detail.update(Message::TitleChanged("  Buy oat milk  ".into()));
        let todo = saved(&mut detail);
        assert_eq!(todo.uid, "t1");
        assert_eq!(todo.href.as_deref(), Some("/cal/work/t1.ics"));
        assert_eq!(todo.etag.as_deref(), Some("\"v7\""));
        assert_eq!(todo.collection, "work");
        assert_eq!(todo.summary, "Buy oat milk");
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
    fn chosen_priority_is_saved() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        detail.update(Message::PrioritySelected(Priority::Low));
        assert_eq!(saved(&mut detail).priority, Some(9));
    }

    #[test]
    fn typed_date_becomes_a_whole_day() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        set_due(&mut detail, "12.10.2026");
        assert_eq!(saved(&mut detail).due, Some(EventTime::Date(day(12))));
        set_due(&mut detail, "");
        assert_eq!(saved(&mut detail).due, None);
    }

    #[test]
    fn date_time_due_keeps_its_time_when_the_date_changes() {
        let mut task = todo("t1", "a");
        task.due = Some(EventTime::DateTime(datetime::to_utc(at(11, 14, 30))));
        let mut detail = detail(&task, &[]);
        set_due(&mut detail, "12.10.2026");
        match saved(&mut detail).due {
            Some(EventTime::DateTime(instant)) => assert_eq!(datetime::to_local(instant), at(12, 14, 30)),
            other => panic!("expected a date-time due, got {other:?}"),
        }
    }

    #[test]
    fn keywords_and_buttons_set_the_day() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        detail.update(Message::DueTomorrow);
        assert_eq!(saved(&mut detail).due, Some(EventTime::Date(day(12))));
        assert_eq!(detail.due_text, "12.10.2026");
        detail.update(Message::DueToday);
        assert_eq!(saved(&mut detail).due, Some(EventTime::Date(day(11))));
        detail.update(Message::DueRemove);
        assert_eq!(saved(&mut detail).due, None);
        assert!(detail.due_text.is_empty());
    }

    #[test]
    fn buttons_keep_the_time_of_a_date_time_due() {
        let mut task = todo("t1", "a");
        task.due = Some(EventTime::DateTime(datetime::to_utc(at(11, 9, 5))));
        let mut detail = detail(&task, &[]);
        detail.update(Message::DueTomorrow);
        match saved(&mut detail).due {
            Some(EventTime::DateTime(instant)) => assert_eq!(datetime::to_local(instant), at(12, 9, 5)),
            other => panic!("expected a date-time due, got {other:?}"),
        }
    }

    #[test]
    fn unreadable_date_blocks_saving_and_keeps_the_last_due() {
        let mut task = todo("t1", "a");
        task.due = Some(EventTime::Date(day(12)));
        let mut detail = detail(&task, &[]);
        set_due(&mut detail, "31.02.");
        assert!(detail.problem().is_some());
        assert!(matches!(detail.update(Message::Save), Action::None));
        assert_eq!(detail.draft.due, Some(EventTime::Date(day(12))));
    }

    #[test]
    fn empty_title_blocks_saving() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        detail.update(Message::TitleChanged("   ".into()));
        assert!(detail.problem().is_some());
        assert!(matches!(detail.update(Message::Save), Action::None));
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
        let choices = detail.parent_choices();
        let uids: Vec<Option<&str>> = choices.iter().map(|c| c.uid.as_deref()).collect();
        assert_eq!(uids, [None, Some("other")]);
    }

    #[test]
    fn parent_choices_follow_the_draft_list() {
        let task = todo("t1", "a");
        let others = vec![todo("other", "Other"), Todo { collection: "home".into(), ..todo("elsewhere", "B") }];
        let mut detail = detail(&task, &others);
        detail.update(Message::ListSelected(ListChoice { id: "home".into(), name: "Home".into() }));
        let choices = detail.parent_choices();
        let uids: Vec<Option<&str>> = choices.iter().map(|c| c.uid.as_deref()).collect();
        assert_eq!(uids, [None, Some("elsewhere")]);
    }

    #[test]
    fn parent_from_another_list_is_dropped_when_the_list_changes() {
        let mut task = todo("t1", "a");
        task.parent = Some("other".into());
        let others = vec![todo("other", "Other")];
        let mut detail = detail(&task, &others);
        detail.update(Message::ListSelected(ListChoice { id: "home".into(), name: "Home".into() }));
        assert_eq!(saved(&mut detail).parent, None);
        assert_eq!(saved(&mut detail).collection, "home");
    }

    #[test]
    fn unknown_parent_is_kept_and_listed() {
        let mut task = todo("t1", "a");
        task.parent = Some("gone".into());
        let detail = detail(&task, &[]);
        let choices = detail.parent_choices();
        assert_eq!(choices.last().and_then(|c| c.uid.as_deref()), Some("gone"));
    }

    #[test]
    fn done_checkbox_is_saved() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        detail.update(Message::DoneToggled(true));
        assert!(saved(&mut detail).completed);
    }

    #[test]
    fn delete_asks_once_before_deleting() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert!(matches!(detail.update(Message::Delete), Action::None));
        assert!(detail.confirm_delete);
        assert!(matches!(detail.update(Message::CancelDelete), Action::None));
        assert!(!detail.confirm_delete);

        detail.update(Message::Delete);
        match detail.update(Message::ConfirmDelete) {
            Action::Delete(todo) => assert_eq!(todo.uid, "t1"),
            other => panic!("expected a delete, got {other:?}"),
        }
        assert!(!detail.confirm_delete);
    }

    #[test]
    fn close_closes() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert!(matches!(detail.update(Message::Close), Action::Close));
    }
}
