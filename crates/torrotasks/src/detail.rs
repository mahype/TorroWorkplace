//! The detail of one task, in the Linear look of the tasks redesign (R.3 in `docs/tasks/design.md`,
//! mock-up `docs/prototypes/tasks/redesign.html`). GitHub issues and pull requests (a task with a
//! [`Link`] the core filled with [`GitHubDetails`]) show their state, repository, labels, checks
//! and reviews.
//!
//! The detail comes in pieces that the app (`app.rs`) places by the size of the window:
//!
//! ```text
//! Fit::Columns (wide, room for three columns)       Fit::Chips (medium) / Fit::Narrow
//! ┌ head ─────────────────────────────────────────┐ ┌ head ──────────────────────┐
//! │ ○ Work › Parent              ˄ ˅ … ✕          │ │ ○ Work › Parent  ˄ ˅ … ✕   │ (narrow: ‹ Today  ˄ ˅ …)
//! ├ main_column ─────────────────┬ properties_col ┤ ├ main_column ───────────────┤
//! │ ○ Title                      │ Status   ○ Open│ │ ○ Title                    │
//! │   notes                      │ Priority ▂▄▆ …  │ │   [○ Open][▂ High][📅 …]   │ ← property_chips
//! │ ˅ Sub-tasks ━━── 2/4         │ Organisation    │ │   notes                    │
//! │ ˅ Checks / Reviews (PRs)     │ Source          │ │ ˅ Sub-tasks …              │
//! └──────────────────────────────┴────────────────┘ └────────────────────────────┘
//! ```
//!
//! - [`Detail::head`]: the row in the window's header bar (44, line below, split from the
//!   list's head by the app) with the path "List › Parent" (only the real hierarchy), ↑ ↓ to the
//!   previous/next task, "…" (Copy link, Open on GitHub / Open link, Delete task – never for a
//!   GitHub item) and ✕; in narrow windows "‹ <list view>" instead of path and ✕.
//! - [`Detail::main_column`]: title with check or GitHub state, for GitHub the line "state ·
//!   owner/repo#123", the [`Detail::property_chips`] unless the properties column is shown, the
//!   notes, "Sub-tasks" (always, to add one), and for pull requests "Checks" and "Reviews" (the
//!   combined state from [`GitHubDetails`]; single checks and reviewers come with G.2).
//! - [`Detail::properties_column`]: the properties beside the main column (wide), blocks of
//!   label | value rows: Status, Priority, Due · "Organisation" List, Parent, Labels · "Source"
//!   Account, Repo, Assignees, Milestone, "Open on GitHub ↗". Values the model does not know yet
//!   (reminder, repeat) are not shown, and nothing that cannot be set gets a placeholder row.
//! - [`Detail::layer`]: the open property editor or the "…" menu, floating over the window.
//!
//! Editing is as before: a click on a property opens its editor as a floating menu or card (due:
//! Today, Tomorrow, Next week, No date and a field for any date such as `11.10.2026`, `today`).
//! Only tasks in a writable list can be edited ([`Detail::lists`]); GitHub filter lists are
//! read-only, so their items only show.
//!
//! Saving works like Linear: there is no Save button. A change of a property is saved at once;
//! title and notes are saved a moment after typing stops ([`Detail::wants_autosave`]), on Enter
//! in the title and when the detail is left ([`Detail::leave`]). Every save is [`Action::Save`]
//! (the core's `save_todo`). Only one save is under way at a time: the server checks the etag, so
//! the next save has to wait for the new one ([`Detail::stored`]). A title that is empty or a
//! date that cannot be read is not saved; the hint appears only after a save was asked for (Enter,
//! a property change, leaving). A new sub-task is [`Action::Create`].

use std::collections::HashSet;

use chrono::{Days, Local, NaiveDate};
use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{button, checkbox, column, container, row, rule, scrollable, stack, svg, text_editor, text_input};
use iced::{Alignment, Background, Color, Element, Fill, Padding, Theme, border, keyboard};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::panel;
use torro_gui::theme::{self, Palette, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail};
use torro_gui::{datetime, style, typo};
use torro_pim::model::{ChecksState, GitHubDetails, GitHubKind, GitHubState, Link, ReviewState};
use torro_pim::{Collection, EventTime, Todo};

use crate::list;

/// The id of the date field in the due card, so opening the card can focus it.
pub const DUE_INPUT: &str = "detail-due";
/// The id of the "Add sub-task" field. The app counts it among its text fields, so plain keys
/// (↑ ↓ Space) typed there do not move through the list.
pub const SUBTASK_INPUT: &str = "detail-subtask";
/// The one-pixel lines of the design.
const LINE: f32 = size::HAIRLINE;
/// The parent menu shows at most this many entries and scrolls the rest.
const PARENT_MENU_ROWS: f32 = 10.0;
/// Edge of the mark in front of the title (check or GitHub state), and the width of the cell the
/// marks of the sub-task, check and review rows stand in, so all share one axis (mock-up: 20).
const TITLE_MARK: f32 = icon::SIZE + space::XS;
/// Where the text under the title starts: past the title mark and its gap (mock-up `.ind`: 32).
const INDENT: f32 = TITLE_MARK + space::M;
/// Space above each section of the main column (Sub-tasks, Checks, Reviews; mock-up: 28).
const SECTION_GAP: f32 = space::XL + space::XS;

/// How the detail is laid out; the app picks it from the window (`layout::of` and the room left).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Wide with room for three columns (list | main | properties): the properties stand in
    /// [`Detail::properties_column`] beside the main column, which shows no chips.
    Columns,
    /// Medium, and wide without room for the properties column: the properties are chips under
    /// the title ([`Detail::property_chips`]); 32 around the main column.
    Chips,
    /// Narrow: the detail replaces the list. Chips as in [`Fit::Chips`], 16 around the main
    /// column, and the head has "‹ <list view>" instead of the path and ✕.
    Narrow,
}

/// The panel's state.
#[derive(Debug)]
pub struct Detail {
    /// The task as last stored; the base for the next save (keeps uid, href and etag).
    pub original: Todo,
    /// The writable task lists, for moving the task. A task whose list is not among them is
    /// read-only here.
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
    /// The property editor or menu that is open.
    open: Option<Editor>,
    /// Set by "Delete task" in the "…" menu; the menu then asks once more.
    confirm_delete: bool,
    /// A save is under way; the next one waits for its answer (new etag).
    saving: bool,
    /// The last task handed out to be saved, so an unchanged draft is not sent again.
    sent: Option<Todo>,
    /// A save was asked for: from now on problems are shown.
    attempted: bool,
    /// The title typed into "Add sub-task".
    new_subtask: String,
    /// Done sub-tasks are hidden ("Hide completed").
    hide_done: bool,
    /// The sections of the main column that are folded away.
    folded: HashSet<Section>,
}

/// The property editors and the menu that open as a floating layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editor {
    Priority,
    Due,
    List,
    Parent,
    /// The "…" menu in the head.
    More,
}

/// The sections of the main column that fold with their chevron.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Subtasks,
    Checks,
    Reviews,
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
    /// Open a property editor or the "…" menu; a second click closes it.
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
    /// Typing in "Add sub-task".
    SubtaskEdited(String),
    /// Enter in "Add sub-task": create it.
    SubtaskSubmit,
    /// "Hide completed" / "Show completed" in the sub-tasks.
    ToggleHideDone,
    /// Fold or unfold a section of the main column.
    Fold(Section),
    /// Open the task's link in the browser ([`Detail::link_target`]).
    OpenLink,
    /// "Copy link" in the "…" menu.
    CopyLink,
    /// "Delete task" in the "…" menu: ask once more.
    Delete,
    CancelDelete,
    ConfirmDelete,
    /// ↑ in the head: the task above in the list.
    Previous,
    /// ↓ in the head: the task below in the list.
    Next,
    /// Close the detail (✕, "‹ Back").
    Close,
    /// Escape: closes the innermost open thing (editor or menu, then the detail).
    Escape,
}

/// What the app should do after an update.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    /// Save this task (the core's `save_todo`); report the answer with [`Detail::stored`] or
    /// [`Detail::save_failed`].
    Save(Todo),
    /// Save this new task (a sub-task: `uid` empty, `parent` this task); nothing to report back,
    /// the next reload brings it into the detail ([`Detail::refresh`]).
    Create(Todo),
    Delete(Todo),
    /// Close the detail; the app saves [`Detail::leave`] first.
    Close,
    /// Tick or untick the task with this uid (a subtask), as the list does.
    ToggleDone(String),
    /// Show the task with this uid in the detail instead.
    Show(String),
    /// Give the keyboard focus to the widget with this id.
    Focus(&'static str),
    /// Open this `http`/`https` address in the browser.
    OpenUrl(String),
    /// Put this address on the clipboard.
    CopyLink(String),
    /// Show the task above the open one in the list (as ↑ moves the cursor), if there is one.
    Previous,
    /// Show the task below the open one in the list (as ↓ moves the cursor), if there is one.
    Next,
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

    /// The short name, for the menu, the chips and the properties.
    fn label(self) -> &'static str {
        match self {
            Priority::None => tr("No priority"),
            Priority::Low => tr("Low"),
            Priority::Medium => tr("Medium"),
            Priority::High => tr("High"),
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

/// One property of the task: a row of [`Detail::properties_column`] or a chip of
/// [`Detail::property_chips`]. Data first, so tests can check which rows show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prop {
    Status,
    Priority,
    Due,
    List,
    Parent,
    Labels,
    Account,
    Repo,
    Assignees,
    Milestone,
    /// "Open on GitHub ↗" or "Open link".
    OpenLink,
}

/// An entry of the "…" menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum More {
    CopyLink,
    /// "Open on GitHub" for a GitHub link, else "Open link".
    OpenLink,
    Delete,
}

/// How a GitHub state looks: its icon, its word and the colour of the icon.
#[derive(Debug, Clone, Copy)]
struct Look {
    icon: Icon,
    word: &'static str,
    colour: fn(&Palette) -> Color,
}

impl Look {
    fn new(icon: Icon, word: &'static str, colour: fn(&Palette) -> Color) -> Look {
        Look { icon, word, colour }
    }
}

/// The state of an issue or pull request (design skill, icons): open green, draft neutral,
/// merged and closed issues violet, closed pull requests red.
fn state_look(details: &GitHubDetails) -> Look {
    match (details.kind, details.state) {
        (GitHubKind::Issue, GitHubState::Open) => Look::new(Icon::CircleDot, tr("Open"), |p| p.ok),
        (GitHubKind::Issue, _) => Look::new(Icon::CircleCheck, tr("Closed"), |p| p.merged),
        (GitHubKind::PullRequest, GitHubState::Open) if details.draft => {
            Look::new(Icon::GitPullRequestDraft, tr("Draft"), |p| p.text_3)
        }
        (GitHubKind::PullRequest, GitHubState::Open) => Look::new(Icon::GitPullRequest, tr("Open"), |p| p.ok),
        (GitHubKind::PullRequest, GitHubState::Merged) => Look::new(Icon::GitMerge, tr("Merged"), |p| p.merged),
        (GitHubKind::PullRequest, GitHubState::Closed) => {
            Look::new(Icon::GitPullRequestClosed, tr("Closed"), |p| p.error)
        }
    }
}

/// The combined checks of a pull request.
fn checks_look(checks: ChecksState) -> Look {
    match checks {
        ChecksState::Pending => Look::new(Icon::CircleDotDashed, tr("Checks running"), |p| p.attention),
        ChecksState::Success => Look::new(Icon::CircleCheck, tr("Checks passed"), |p| p.ok),
        ChecksState::Failure => Look::new(Icon::CircleX, tr("Checks failed"), |p| p.error),
    }
}

/// GitHub's review decision on a pull request.
fn review_look(review: ReviewState) -> Look {
    match review {
        ReviewState::Required => Look::new(Icon::Eye, tr("Review required"), |p| p.attention),
        ReviewState::Approved => Look::new(Icon::CircleCheck, tr("Approved"), |p| p.ok),
        ReviewState::ChangesRequested => Look::new(Icon::CircleAlert, tr("Changes requested"), |p| p.attention),
    }
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
            new_subtask: String::new(),
            hide_done: false,
            folded: HashSet::new(),
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
            Message::CloseEditor => self.close_editor(),
            Message::PrioritySelected(priority) => {
                self.close_editor();
                self.draft.priority = priority;
                return self.request_save(true);
            }
            Message::ListSelected(id) => {
                self.close_editor();
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
                self.close_editor();
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
                    self.close_editor();
                }
                return self.request_save(true);
            }
            Message::DueToday => return self.pick_due(Some(self.today)),
            Message::DueTomorrow => return self.pick_due(Some(self.today + Days::new(1))),
            Message::DueNextWeek => return self.pick_due(Some(next_week(self.today))),
            Message::DueRemove => return self.pick_due(None),
            Message::ToggleSubtask(uid) => return Action::ToggleDone(uid),
            Message::OpenSubtask(uid) => return Action::Show(uid),
            Message::SubtaskEdited(title) => self.new_subtask = title,
            Message::SubtaskSubmit => return self.create_subtask(),
            Message::ToggleHideDone => self.hide_done = !self.hide_done,
            Message::Fold(section) => {
                if !self.folded.remove(&section) {
                    self.folded.insert(section);
                }
            }
            Message::OpenLink => {
                self.close_editor();
                return self.link_target().map_or(Action::None, Action::OpenUrl);
            }
            Message::CopyLink => {
                self.close_editor();
                return self.link_target().map_or(Action::None, Action::CopyLink);
            }
            Message::Delete => self.confirm_delete = self.can_delete(),
            Message::CancelDelete => self.close_editor(),
            Message::ConfirmDelete => {
                self.close_editor();
                if self.can_delete() {
                    return Action::Delete(self.original.clone());
                }
            }
            Message::Previous => {
                self.close_editor();
                return Action::Previous;
            }
            Message::Next => {
                self.close_editor();
                return Action::Next;
            }
            Message::Close => return Action::Close,
            Message::Escape => {
                if self.open.is_some() {
                    self.close_editor();
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
        self.close_editor();
        self.draft.due = day.map(|day| due_with_date(self.draft.due, day));
        self.due_text = due_input(self.draft.due);
        self.request_save(true)
    }

    /// Closes the open editor or menu, and with it the delete question.
    fn close_editor(&mut self) {
        self.open = None;
        self.confirm_delete = false;
    }

    /// A new open sub-task with the typed title in this task's list; the field empties for the
    /// next one. Nothing for an empty title or a task that cannot be edited.
    fn create_subtask(&mut self) -> Action {
        let summary = self.new_subtask.trim().to_string();
        if summary.is_empty() || !self.editable() {
            return Action::None;
        }
        self.new_subtask.clear();
        Action::Create(Todo {
            uid: String::new(),
            collection: self.draft.collection.clone(),
            href: None,
            etag: None,
            summary,
            due: None,
            priority: None,
            completed: false,
            parent: Some(self.original.uid.clone()),
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
        })
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
    // What the task is
    // -----------------------------------------------------------------------------------------

    /// What GitHub reported for the item; `None` for a plain task (or a link not fetched yet).
    fn github(&self) -> Option<&GitHubDetails> {
        self.original.link.as_ref()?.github.as_ref()
    }

    /// `("owner/name", number)` of a GitHub link, fetched or not.
    fn github_ref(&self) -> Option<(&str, u64)> {
        self.original.link.as_ref()?.github_ref()
    }

    /// The task's list among the writable ones; `None` for a read-only list (GitHub filters).
    fn list(&self) -> Option<&Collection> {
        self.lists.iter().find(|c| c.id == self.draft.collection)
    }

    /// The task can be changed: its list is writable.
    fn editable(&self) -> bool {
        self.list().is_some()
    }

    /// Deleting is for tasks only: a GitHub item lives on GitHub (and in a read-only list).
    fn can_delete(&self) -> bool {
        self.editable() && self.github().is_none()
    }

    /// Moving needs another writable list.
    fn can_move(&self) -> bool {
        self.editable() && self.lists.len() > 1
    }

    /// Where the task lives, for the path and the List row: the list's name, else for a GitHub
    /// item its repository's name (as GitHub shows it, without the owner).
    fn place_name(&self) -> String {
        match (self.list(), self.github_ref()) {
            (Some(list), _) => list.name.clone(),
            (None, Some((repo, _))) => repo.rsplit_once('/').map_or(repo, |(_, name)| name).to_string(),
            (None, None) => tr("List").to_string(),
        }
    }

    /// The mark of the place: the list's ring, else the GitHub logo.
    fn place_mark(&self) -> Mark {
        match (self.list(), self.github_ref()) {
            (None, Some(_)) => Mark::Icon(Icon::Github),
            _ => Mark::Ring(self.list_colour()),
        }
    }

    fn list_colour(&self) -> Color {
        widgets::collection_color(self.list().and_then(|c| c.color.as_deref()))
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

    fn status_word(&self) -> &'static str {
        match self.github() {
            Some(details) => state_look(details).word,
            None if self.draft.completed => tr("Completed"),
            None => tr("Open"),
        }
    }

    fn due_label(&self) -> Option<String> {
        self.draft.due.map(|due| list::due_text(due, self.today))
    }

    /// An open task whose day lies before today (shown in the attention colour).
    fn overdue(&self) -> bool {
        !self.draft.completed && self.draft.due.is_some_and(|due| due_day(due) < self.today)
    }

    /// The labels as `(colour, name)`: GitHub's labels with their colours, else the task's tags
    /// (no colour: a neutral dot). Empty names are left out.
    fn labels(&self) -> Vec<(String, String)> {
        let github = self.github().map(|d| &d.labels).filter(|labels| !labels.is_empty());
        let labels: Vec<(String, String)> = match github {
            Some(labels) => labels.iter().map(|l| (l.color.clone().unwrap_or_default(), l.name.clone())).collect(),
            None => self.original.categories.iter().map(|tag| (String::new(), tag.clone())).collect(),
        };
        labels
            .into_iter()
            .map(|(colour, name)| (colour, name.trim().to_string()))
            .filter(|(_, name)| !name.is_empty())
            .collect()
    }

    /// The address "Open link" opens: the task's `url`, else the page of its link (a GitHub
    /// issue). Only `http` and `https` are opened; anything else (`file:`, an app's own scheme)
    /// could start programs, so it is not offered.
    fn link_target(&self) -> Option<String> {
        let web = |url: &str| {
            let lower = url.trim().to_ascii_lowercase();
            (lower.starts_with("https://") || lower.starts_with("http://")).then(|| url.trim().to_string())
        };
        let own = self.original.url.as_deref().and_then(web);
        own.or_else(|| self.original.link.as_ref().and_then(Link::web_url).as_deref().and_then(web))
    }

    /// The blocks of the properties column, top to bottom: an optional title and its rows.
    /// Rows the task cannot have are left out; a row that is not set stays (with a placeholder)
    /// only when a click can set it.
    fn property_blocks(&self, account: bool) -> Vec<(Option<&'static str>, Vec<Prop>)> {
        let editable = self.editable();
        let mut first = vec![Prop::Status];
        if editable || self.draft.priority != Priority::None {
            first.push(Prop::Priority);
        }
        if editable || self.draft.due.is_some() {
            first.push(Prop::Due);
        }
        let mut organisation = vec![Prop::List];
        if editable || self.draft.parent.is_some() {
            organisation.push(Prop::Parent);
        }
        if !self.labels().is_empty() {
            organisation.push(Prop::Labels);
        }
        let mut source = Vec::new();
        if account {
            source.push(Prop::Account);
        }
        if self.github_ref().is_some() {
            source.push(Prop::Repo);
        }
        if let Some(details) = self.github() {
            if !details.assignees.is_empty() {
                source.push(Prop::Assignees);
            }
            if details.milestone.is_some() {
                source.push(Prop::Milestone);
            }
        }
        if self.link_target().is_some() {
            source.push(Prop::OpenLink);
        }
        let mut blocks = vec![(None, first), (Some(tr("Organisation")), organisation)];
        if !source.is_empty() {
            blocks.push((Some(tr("Source")), source));
        }
        blocks
    }

    /// The chips under the title: status (tasks only; a GitHub item shows its state as a pill
    /// already), priority, due and list – what the properties column starts with.
    fn chip_props(&self) -> Vec<Prop> {
        let editable = self.editable();
        let mut chips = Vec::new();
        if self.github().is_none() {
            chips.push(Prop::Status);
        }
        if editable || self.draft.priority != Priority::None {
            chips.push(Prop::Priority);
        }
        if editable || self.draft.due.is_some() {
            chips.push(Prop::Due);
        }
        chips.push(Prop::List);
        chips
    }

    /// The entries of the "…" menu; empty when there is nothing to do (then there is no "…").
    fn more_items(&self) -> Vec<More> {
        let mut items = Vec::new();
        if self.link_target().is_some() {
            items.extend([More::CopyLink, More::OpenLink]);
        }
        if self.can_delete() {
            items.push(More::Delete);
        }
        items
    }

    // -----------------------------------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------------------------------

    /// The head of the detail: the row that stands in the window's header bar over the main
    /// column and – in [`Fit::Columns`] – the properties column next to it. The app puts it there
    /// (`app.rs`, R.2), so the header bar gives it its 44 height, its padding (16 left, 12 right;
    /// 8 in narrow windows) and the line below, in line with the head of the list beside it.
    ///
    /// Left the path "○ Work › Parent task" (the list, or a GitHub item's repository, then the
    /// parent if there is one), right ↑ ↓ ([`Action::Previous`] / [`Action::Next`]), "…" (the
    /// menu floats in [`Detail::layer`]) and ✕ ([`Action::Close`]). In [`Fit::Narrow`] the path
    /// and ✕ give way to "‹ `back`" (closes, back to the list); `back` is the name of the list
    /// view the detail was opened from ("Today"). `anchor` is the one passed to
    /// [`Detail::layer`].
    pub fn head<'a>(&'a self, fit: Fit, back: String, anchor: &Anchor) -> Element<'a, Message> {
        let narrow = fit == Fit::Narrow;
        let mut bar = if narrow {
            let back = widgets::text_button(Some(Icon::ChevronLeft), back, style::button::ghost, Some(Message::Close));
            row![back, blank::horizontal()]
        } else {
            row![container(self.path()).width(Fill).clip(true)]
        };
        bar = bar.extend(self.head_buttons(anchor));
        if !narrow {
            bar = bar.push(widgets::icon_button(Icon::X, tr("Close"), Some(Message::Close)));
        }
        bar.spacing(space::S).align_y(Alignment::Center).into()
    }

    /// The main column of the detail, filling its room and scrolling: 32 around (16 in
    /// [`Fit::Narrow`]), the content at most `FORM_MAX` (640) wide and left-aligned.
    ///
    /// Top to bottom: the title (check or GitHub state 20, `HEADLINE`, edited in place); for a
    /// GitHub item the state pill and `owner/repo#123`; the [`Detail::property_chips`] unless
    /// `fit` is [`Fit::Columns`]; the notes; "Sub-tasks" with progress, "Hide completed" and
    /// "Add sub-task"; for pull requests "Checks" and "Reviews". `palette` is the current
    /// theme's (`palette(&self.theme())`), for the coloured state icons in chips and pill.
    pub fn main_column<'a>(&'a self, fit: Fit, palette: &'static Palette, anchor: &Anchor) -> Element<'a, Message> {
        let content = column![self.title_block(), self.body(fit, Some(palette), anchor)].max_width(size::FORM_MAX);
        let around = if fit == Fit::Narrow { space::L } else { space::XXL };
        scrollable(container(content).width(Fill).padding(around))
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// The properties column of the wide layout: `PROPS` (300) wide, full height, scrolling;
    /// it draws no line of its own, so put one `LINE` between it and the main column.
    ///
    /// Blocks separated by lines: Status, Priority, Due · "Organisation": List, Parent, Labels ·
    /// "Source": Account, Repo, Assignees, Milestone, "Open on GitHub ↗" (or "Open link").
    /// `account` is the name of the account the task's list belongs to (the sidebar's group
    /// title, `torro_pim::account::account_of`); `None` leaves the row out. A click on Status,
    /// Priority, Due, List or Parent edits it; the editors float in [`Detail::layer`] next to
    /// `anchor`.
    pub fn properties_column<'a>(&'a self, account: Option<String>, anchor: &Anchor) -> Element<'a, Message> {
        let mut blocks = column![];
        for (i, (title, props)) in self.property_blocks(account.is_some()).into_iter().enumerate() {
            if i > 0 {
                blocks = blocks.push(rule::horizontal(LINE).style(style::rule));
            }
            let mut block = column![];
            if let Some(title) = title {
                block = block.push(
                    container(typo::text(typo::CAPTION, title).style(theme::text_3))
                        .center_y(size::CONTROL_SM)
                        .padding(style::ITEM_PADDING),
                );
            }
            block = block.extend(props.into_iter().map(|prop| self.prop_row(prop, account.as_deref(), anchor)));
            // 8 around the rows, whose own 8 bring their text to 16 from the column's edge.
            blocks = blocks.push(container(block).width(Fill).padding(space::S));
        }
        let content = container(blocks).width(Fill).padding(Padding::new(0.0).bottom(space::XL));
        container(
            scrollable(content).direction(scrollable::Direction::Vertical(style::scrollbar())).style(style::scrollable),
        )
        .width(size::PROPS)
        .height(Fill)
        .into()
    }

    /// The properties as chips that wrap (medium and narrow, and wide without room for the
    /// properties column): status (tasks only), priority, due, list. A click opens the same
    /// editor as the properties column. [`Detail::main_column`] already shows them under the
    /// title when `fit` is not [`Fit::Columns`]; call this only to place them elsewhere.
    #[cfg_attr(not(test), expect(dead_code, reason = "main_column shows the chips; for placing them elsewhere"))]
    pub fn property_chips<'a>(&'a self, palette: &'static Palette, anchor: &Anchor) -> Element<'a, Message> {
        self.chips_view(Some(palette), anchor)
    }

    /// The open editor or menu as a floating layer next to `anchor`, or `None`. Hand it to the
    /// shell's overlay; a click beside it sends [`Message::CloseEditor`].
    pub fn layer<'a>(&'a self, anchor: &Anchor) -> Option<Element<'a, Message>> {
        let (card, align) = match self.open? {
            Editor::Priority => (self.priority_menu(), Align::Start),
            Editor::List => (self.list_menu(), Align::Start),
            Editor::Parent => (self.parent_menu(), Align::Start),
            Editor::Due => (self.due_card(), Align::Start),
            Editor::More => (self.more_menu(), Align::End),
        };
        Some(widgets::floating(anchor, Placement::Below(align), card, Message::CloseEditor))
    }

    /// "○ Work › Parent task": the place and, for a sub-task, its parent.
    fn path<'a>(&self) -> Element<'a, Message> {
        let parts = std::iter::once(self.place_name()).chain(self.parent_title());
        panel::path(Some(self.place_mark()), parts)
    }

    /// ↑, ↓ and "…" (when its menu has entries).
    fn head_buttons<'a>(&'a self, anchor: &Anchor) -> Vec<Element<'a, Message>> {
        let mut buttons = vec![
            widgets::icon_button(Icon::ChevronUp, tr("Previous task"), Some(Message::Previous)),
            widgets::icon_button(Icon::ChevronDown, tr("Next task"), Some(Message::Next)),
        ];
        if !self.more_items().is_empty() {
            let more = widgets::icon_button(Icon::Ellipsis, tr("More"), Some(Message::Open(Editor::More)));
            buttons.push(self.trigger(Editor::More, anchor, more));
        }
        buttons
    }

    /// The title row: the check (or the GitHub state) 20 on the axis of the first line, then
    /// the title in `HEADLINE`; the hint for an empty title below it.
    fn title_block<'a>(&'a self) -> Element<'a, Message> {
        let mark: Element<'a, Message> = match self.github() {
            Some(details) => {
                let look = state_look(details);
                tinted_icon(look.icon, TITLE_MARK, look.colour)
            }
            None => self.check(TITLE_MARK),
        };
        let mark = container(mark).center_x(TITLE_MARK).center_y(typo_line(typo::HEADLINE));
        let title: Element<'a, Message> = if self.editable() {
            self.title_field()
        } else {
            typo::text(typo::HEADLINE, self.draft.summary.clone()).wrapping(Wrapping::WordOrGlyph).into()
        };
        let mut block = column![row![mark, container(title).width(Fill)].spacing(space::M)];
        if let Some(problem) = self.shown(self.title_problem()) {
            block = block.push(indented(error_text(problem)));
        }
        block.spacing(space::XS).into()
    }

    /// Everything under the title, at the title's left edge. `colours` is the current theme's
    /// palette for coloured marks; `None` (the panel) shows them neutral.
    fn body<'a>(&'a self, fit: Fit, colours: Option<&'static Palette>, anchor: &Anchor) -> Element<'a, Message> {
        let mut body = column![];
        if let Some(meta) = self.github_meta(colours) {
            body = body.push(gap(space::S, indented(meta)));
        }
        if fit != Fit::Columns {
            body = body.push(gap(space::L, indented(self.chips_view(colours, anchor))));
        }
        if let Some(notes) = self.notes_view() {
            body = body.push(gap(space::M, notes));
        }
        let sections = [self.subtasks_section(), self.checks_section(), self.reviews_section()];
        body.extend(sections.into_iter().flatten().map(|section| gap(SECTION_GAP, section))).into()
    }

    /// "◉ Open · owner/repo#123" under a GitHub item's title; `None` for a plain task.
    fn github_meta<'a>(&'a self, colours: Option<&'static Palette>) -> Option<Element<'a, Message>> {
        let look = state_look(self.github()?);
        let mut line =
            row![widgets::state_pill(look_mark(colours, look), look.word)].spacing(space::S).align_y(Alignment::Center);
        if let Some((repo, number)) = self.github_ref() {
            line = line.push(typo::text(typo::META.tabular(), format!("{repo}#{number}")).style(theme::text_3));
        }
        Some(line.wrap().vertical_spacing(space::XS).into())
    }

    fn chips_view<'a>(&'a self, colours: Option<&'static Palette>, anchor: &Anchor) -> Element<'a, Message> {
        let chips = self.chip_props().into_iter().map(|prop| self.chip(prop, colours, anchor)).collect();
        widgets::chip_row(chips)
    }

    /// One property as a chip.
    fn chip<'a>(&'a self, prop: Prop, colours: Option<&'static Palette>, anchor: &Anchor) -> Element<'a, Message> {
        let editable = self.editable();
        let (mark, label, editor) = match prop {
            Prop::Status => {
                let mark = if self.draft.completed {
                    Mark::Tinted(Icon::CircleCheck, self.list_colour())
                } else {
                    Mark::Ring(colours.map_or(self.list_colour(), |p| p.text_3))
                };
                let on_press = editable.then_some(Message::ToggleDone);
                return widgets::chip(Some(mark), self.status_word(), on_press);
            }
            Prop::Priority => {
                let label = match self.draft.priority {
                    Priority::None => tr("Priority"),
                    set => set.label(),
                };
                (Mark::Icon(Icon::SignalHigh), label.to_string(), Editor::Priority)
            }
            Prop::Due => {
                let mark = match colours {
                    Some(p) if self.overdue() => Mark::Tinted(Icon::Calendar, p.attention_text),
                    _ => Mark::Icon(Icon::Calendar),
                };
                (mark, self.due_label().unwrap_or_else(|| tr("Due date").to_string()), Editor::Due)
            }
            Prop::List => {
                let on_press = self.can_move().then_some(Message::Open(Editor::List));
                let chip = widgets::chip(Some(self.place_mark()), self.place_name(), on_press);
                return self.trigger(Editor::List, anchor, chip);
            }
            _ => return blank::horizontal().width(0).into(),
        };
        let chip = widgets::chip(Some(mark), label, editable.then_some(Message::Open(editor)));
        self.trigger(editor, anchor, chip)
    }

    /// One row of the properties column.
    fn prop_row<'a>(&'a self, prop: Prop, account: Option<&str>, anchor: &Anchor) -> Element<'a, Message> {
        let editable = self.editable();
        let open = |editor| editable.then_some(Message::Open(editor));
        match prop {
            Prop::Status => {
                let (mark, on_press) = match self.github() {
                    Some(details) => {
                        let look = state_look(details);
                        (tinted_icon(look.icon, icon::SIZE, look.colour), None)
                    }
                    None => (self.check(style::CHECK_SIZE), editable.then_some(Message::ToggleDone)),
                };
                let value = row![mark, widgets::prop_value(self.status_word())].spacing(space::S);
                widgets::prop_line(tr("Status"), None, value.align_y(Alignment::Center), on_press)
            }
            Prop::Priority => {
                let line = match self.draft.priority {
                    Priority::None => widgets::prop_line(
                        tr("Priority"),
                        Some(Mark::Icon(Icon::SignalHigh)),
                        widgets::prop_placeholder(tr("Add priority")),
                        open(Editor::Priority),
                    ),
                    set => {
                        let value = row![bars_cell(set.level()), widgets::prop_value(set.label())]
                            .spacing(space::S)
                            .align_y(Alignment::Center);
                        widgets::prop_line(tr("Priority"), None, value, open(Editor::Priority))
                    }
                };
                self.trigger(Editor::Priority, anchor, line)
            }
            Prop::Due => {
                let value: Element<'a, Message> = match self.due_label() {
                    Some(label) => due_value(label, self.overdue()),
                    None => widgets::prop_placeholder(tr("Add due date")),
                };
                let line = widgets::prop_line(tr("Due"), Some(Mark::Icon(Icon::Calendar)), value, open(Editor::Due));
                self.trigger(Editor::Due, anchor, line)
            }
            Prop::List => {
                let on_press = self.can_move().then_some(Message::Open(Editor::List));
                let line = widgets::prop_line(
                    tr("List"),
                    Some(self.place_mark()),
                    widgets::prop_value(self.place_name()),
                    on_press,
                );
                self.trigger(Editor::List, anchor, line)
            }
            Prop::Parent => {
                let value = match self.parent_title() {
                    Some(title) => widgets::prop_value(title),
                    None => widgets::prop_placeholder(tr("Add parent task")),
                };
                let line =
                    widgets::prop_line(tr("Parent"), Some(Mark::Icon(Icon::ListTree)), value, open(Editor::Parent));
                self.trigger(Editor::Parent, anchor, line)
            }
            Prop::Labels => {
                let chips =
                    self.labels().into_iter().map(|(colour, name)| widgets::label_chip(&colour, name)).collect();
                widgets::prop_line(tr("Labels"), None, widgets::label_chip_row(chips), None)
            }
            Prop::Account => {
                // A GitHub account's lists have ids starting with its id `github:<login>`.
                let github = self.original.collection.starts_with("github:");
                let mark = if github { Icon::Github } else { Icon::Cloud };
                let name = account.unwrap_or_default().to_string();
                widgets::prop_line(tr("Account"), Some(Mark::Icon(mark)), widgets::prop_value(name), None)
            }
            Prop::Repo => {
                let repo = self.github_ref().map(|(repo, _)| repo.to_string()).unwrap_or_default();
                widgets::prop_line(tr("Repo"), Some(Mark::Icon(Icon::FolderGit2)), widgets::prop_value(repo), None)
            }
            Prop::Assignees => {
                let names = self.github().map(|d| d.assignees.join(", ")).unwrap_or_default();
                widgets::prop_line(tr("Assignees"), Some(Mark::Icon(Icon::User)), widgets::prop_value(names), None)
            }
            Prop::Milestone => {
                let name = self.github().and_then(|d| d.milestone.clone()).unwrap_or_default();
                widgets::prop_line(tr("Milestone"), Some(Mark::Icon(Icon::Milestone)), widgets::prop_value(name), None)
            }
            Prop::OpenLink => self.open_link_row(),
        }
    }

    /// "Open on GitHub ↗" for a GitHub link, else "Open link" with where it leads; a row without
    /// a label, as wide as the column.
    fn open_link_row<'a>(&'a self) -> Element<'a, Message> {
        let (mark, label, place) = match self.github_ref() {
            Some(_) => (Icon::Github, tr("Open on GitHub"), None),
            None => {
                let target = self.link_target().unwrap_or_default();
                let place = target.split_once("://").map_or(target.as_str(), |(_, rest)| rest).to_string();
                (Icon::Link, tr("Open link"), Some(place))
            }
        };
        let mut value =
            row![typo::text(typo::BODY, label).wrapping(Wrapping::None)].spacing(space::S).align_y(Alignment::Center);
        value = match place {
            Some(place) => value.push(
                container(typo::text(typo::META, place).style(theme::text_3).wrapping(Wrapping::None))
                    .width(Fill)
                    .clip(true),
            ),
            None => value.push(tinted_icon(Icon::ArrowUpRight, icon::SIZE_SM, |p| p.text_3)),
        };
        widgets::prop_row(Some(Mark::Icon(mark)), value, Some(Message::OpenLink))
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
            .style(|t, s| bare_editor(t, s, palette(t).text, false))
            .into()
    }

    /// The notes without a box: a quiet field in `text_2` with a hover surface whose text lines
    /// up with the title ("Add notes …" while empty). Read-only tasks show their notes as text,
    /// and nothing when there are none.
    fn notes_view(&self) -> Option<Element<'_, Message>> {
        if !self.editable() {
            let notes = self.draft.notes.trim();
            return (!notes.is_empty()).then(|| {
                indented(typo::text(typo::BODY, notes.to_string()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph))
            });
        }
        let field = text_editor(&self.notes)
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
            .padding(style::INPUT_PADDING)
            .wrapping(Wrapping::WordOrGlyph)
            .style(|t, s| bare_editor(t, s, palette(t).text_2, true));
        // The field's own 8 at the left bring its text onto the indent of the title.
        Some(container(field).padding(Padding::new(0.0).left(INDENT - style::INPUT_PADDING.left)).into())
    }

    /// `content` anchored when `editor` is the open one, so its layer floats next to it.
    fn trigger<'a>(&self, editor: Editor, anchor: &Anchor, content: Element<'a, Message>) -> Element<'a, Message> {
        if self.open == Some(editor) { widgets::anchored(anchor, content) } else { content }
    }

    /// The round check of the task itself, in the list's colour, `edge` wide; it ticks only
    /// where the task can be edited.
    fn check(&self, edge: f32) -> Element<'_, Message> {
        let check = checkbox(self.draft.completed).size(edge).style(style::task_check(self.list_colour()));
        if self.editable() { check.on_toggle(|_| Message::ToggleDone).into() } else { check.into() }
    }

    /// The head of a section: chevron and title (one button that folds the section), then
    /// `extra` (progress, "Hide completed").
    fn section_head<'a>(
        &self,
        section: Section,
        title: &'static str,
        extra: Vec<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let chevron = if self.folded.contains(&section) { Icon::ChevronRight } else { Icon::ChevronDown };
        let label = row![
            container(tinted_icon(chevron, icon::SIZE_SM, |p| p.text_3)).center_x(TITLE_MARK),
            typo::text(typo::CAPTION, title).wrapping(Wrapping::None),
        ]
        .spacing(space::M)
        .align_y(Alignment::Center);
        // Centred in the button, so the title stands on one line with progress and "Hide completed".
        let fold = button(container(label).center_y(Fill))
            .padding(Padding::new(0.0).right(space::XS))
            .height(size::CONTROL_SM)
            .style(style::button::ghost)
            .on_press(Message::Fold(section));
        container(row![container(fold).center_y(Fill)].extend(extra).spacing(space::M).align_y(Alignment::Center))
            .width(Fill)
            .height(size::CONTROL)
            .into()
    }

    /// "Sub-tasks": progress, the direct sub-tasks (done ones hidden on request) and "Add
    /// sub-task", which is always there where the task can be edited. `None` for a read-only task
    /// without sub-tasks.
    fn subtasks_section(&self) -> Option<Element<'_, Message>> {
        let subtasks = self.subtasks();
        if subtasks.is_empty() && !self.editable() {
            return None;
        }
        let done = subtasks.iter().filter(|t| t.completed).count();
        let mut extra: Vec<Element<'_, Message>> = Vec::new();
        if !subtasks.is_empty() {
            extra.push(widgets::progress_bar(done, subtasks.len()));
            extra.push(
                typo::text(typo::META.tabular(), format!("{done}/{}", subtasks.len())).style(theme::text_3).into(),
            );
        }
        extra.push(blank::horizontal().into());
        if done > 0 {
            let label = if self.hide_done { tr("Show completed") } else { tr("Hide completed") };
            extra.push(widgets::text_button(None, label, style::button::ghost, Some(Message::ToggleHideDone)));
        }
        let mut section = column![self.section_head(Section::Subtasks, tr("Sub-tasks"), extra)];
        if self.folded.contains(&Section::Subtasks) {
            return Some(section.into());
        }
        let colour = self.list_colour();
        let shown = subtasks.into_iter().filter(|t| !(self.hide_done && t.completed));
        section = section.extend(shown.map(|task| subtask_row(task, colour)));
        if self.editable() {
            section = section.push(self.add_subtask_row());
        }
        Some(section.into())
    }

    /// "+ Add sub-task": a field that looks like a row; Enter creates the sub-task.
    fn add_subtask_row(&self) -> Element<'_, Message> {
        let field = text_input(tr("Add sub-task"), &self.new_subtask)
            .id(SUBTASK_INPUT)
            .on_input(Message::SubtaskEdited)
            .on_submit(Message::SubtaskSubmit)
            .padding(Padding::new(0.0).left(INDENT).right(space::S))
            .font(typo::BODY.font)
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .style(bare_input);
        let plus = container(tinted_icon(Icon::Plus, icon::SIZE_SM, |p| p.text_3)).center_x(TITLE_MARK).center_y(Fill);
        // The "+" lies over the field, so the whole row is the field (hover, click to type).
        container(stack![container(field).center_y(Fill), plus]).height(size::CONTROL).into()
    }

    /// "Checks" of a pull request: the combined state (and merge conflicts); `None` when GitHub
    /// reports neither.
    fn checks_section(&self) -> Option<Element<'_, Message>> {
        let details = self.github().filter(|d| d.kind == GitHubKind::PullRequest)?;
        let mut rows: Vec<Element<'_, Message>> =
            details.checks.map(|c| look_row(checks_look(c))).into_iter().collect();
        if details.conflict {
            rows.push(look_row(Look::new(Icon::CircleAlert, tr("Merge conflicts"), |p| p.attention)));
        }
        if rows.is_empty() {
            return None;
        }
        let mut section = column![self.section_head(Section::Checks, tr("Checks"), Vec::new())];
        if !self.folded.contains(&Section::Checks) {
            section = section.extend(rows);
        }
        Some(section.into())
    }

    /// "Reviews" of a pull request: GitHub's review decision; `None` without one.
    fn reviews_section(&self) -> Option<Element<'_, Message>> {
        let review = self.github().filter(|d| d.kind == GitHubKind::PullRequest)?.review?;
        let mut section = column![self.section_head(Section::Reviews, tr("Reviews"), Vec::new())];
        if !self.folded.contains(&Section::Reviews) {
            section = section.push(look_row(review_look(review)));
        }
        Some(section.into())
    }

    /// The direct subtasks, open first, then by title.
    fn subtasks(&self) -> Vec<&Todo> {
        let mut subtasks: Vec<&Todo> =
            self.others.iter().filter(|t| t.parent.as_deref() == Some(self.original.uid.as_str())).collect();
        subtasks.sort_by_key(|t| (t.completed, t.summary.to_lowercase()));
        subtasks
    }

    /// The "…" menu: Copy link and Open on GitHub / Open link, then Delete task; after "Delete
    /// task" the last part asks once more.
    fn more_menu(&self) -> Element<'_, Message> {
        let mut first = Vec::new();
        let mut sections = Vec::new();
        for item in self.more_items() {
            match item {
                More::CopyLink => first.push(MenuItem::new(Icon::Link, tr("Copy link"), Message::CopyLink)),
                More::OpenLink => {
                    let (mark, label) = match self.github_ref() {
                        Some(_) => (Icon::Github, tr("Open on GitHub")),
                        None => (Icon::Link, tr("Open link")),
                    };
                    first.push(MenuItem::new(mark, label, Message::OpenLink).trail(Trail::Icon(Icon::ArrowUpRight)));
                }
                More::Delete if self.confirm_delete => sections.push(MenuSection::titled(
                    tr("Delete this task?"),
                    vec![
                        MenuItem::new(Icon::Trash2, tr("Delete task"), Message::ConfirmDelete),
                        MenuItem::new(Icon::X, tr("Cancel"), Message::CancelDelete),
                    ],
                )),
                More::Delete => sections.push(MenuSection::new(vec![MenuItem::new(
                    Icon::Trash2,
                    tr("Delete task"),
                    Message::Delete,
                )])),
            }
        }
        if !first.is_empty() {
            sections.insert(0, MenuSection::new(first));
        }
        widgets::menu(sections)
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

/// One subtask, 32 high with a line below: the round check in the list colour on the axis of the
/// title's check, then the title (struck through when done) as a button that opens it.
fn subtask_row(task: &Todo, colour: Color) -> Element<'_, Message> {
    let uid = task.uid.clone();
    let check = checkbox(task.completed)
        .size(style::CHECK_SIZE)
        .style(style::task_check(colour))
        .on_toggle(move |_| Message::ToggleSubtask(uid.clone()));
    let title = button(container(list::title_view(task)).width(Fill).clip(true).center_y(Fill))
        .width(Fill)
        .height(size::CONTROL - LINE)
        .padding(Padding::new(0.0).right(space::S))
        .style(style::button::list_row(false))
        .on_press(Message::OpenSubtask(task.uid.clone()));
    let line = row![container(check).center_x(TITLE_MARK), title].spacing(space::M).align_y(Alignment::Center);
    column![line, rule::horizontal(LINE).style(style::rule)].into()
}

/// A row of the Checks or Reviews section: the state's icon in its colour on the axis of the
/// title's mark, then its word; 32 high with a line below.
fn look_row<'a>(look: Look) -> Element<'a, Message> {
    let line = row![
        container(tinted_icon(look.icon, icon::SIZE, look.colour)).center_x(TITLE_MARK),
        typo::text(typo::BODY, look.word).wrapping(Wrapping::None),
    ]
    .spacing(space::M)
    .align_y(Alignment::Center);
    column![container(line).center_y(size::CONTROL - LINE), rule::horizontal(LINE).style(style::rule)].into()
}

/// The mark of a GitHub state for a pill or chip: tinted with `colours` (the current palette),
/// neutral without.
fn look_mark(colours: Option<&Palette>, look: Look) -> Mark {
    colours.map_or(Mark::Icon(look.icon), |p| Mark::Tinted(look.icon, (look.colour)(p)))
}

/// `which`, `edge` wide, in the colour `pick` takes from the current theme.
fn tinted_icon<'a>(which: Icon, edge: f32, pick: fn(&Palette) -> Color) -> Element<'a, Message> {
    icon::icon(which, edge).style(move |t, _| svg::Style { color: Some(pick(palette(t))) }).into()
}

/// A due date in a property row: `BODY` tabular, in the attention colour when overdue.
fn due_value<'a>(label: String, overdue: bool) -> Element<'a, Message> {
    let text = typo::text(typo::BODY.tabular(), label).wrapping(Wrapping::None).style(move |t: &Theme| {
        let p = palette(t);
        iced::widget::text::Style { color: Some(if overdue { p.attention_text } else { p.text }) }
    });
    container(text).width(Fill).clip(true).into()
}

/// The line height of `level` in pixels, for a cell that centres a mark on a text's first line.
fn typo_line(level: typo::Level) -> f32 {
    match level.line_height {
        iced::widget::text::LineHeight::Absolute(pixels) => pixels.0,
        iced::widget::text::LineHeight::Relative(factor) => factor * level.size.0,
    }
}

/// `content` moved in to the text of the title (past its mark).
fn indented<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).padding(Padding::new(0.0).left(INDENT)).into()
}

/// `content` with `top` free above it.
fn gap<'a>(top: f32, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).padding(Padding::new(0.0).top(top)).into()
}

/// Whether the editor that got `press` has the keyboard focus.
fn is_focused(press: &text_editor::KeyPress) -> bool {
    matches!(press.status, text_editor::Status::Focused { .. })
}

/// Escape in a text field: leave it and save what was typed.
fn leave_field() -> text_editor::Binding<Message> {
    text_editor::Binding::Sequence(vec![text_editor::Binding::Unfocus, text_editor::Binding::Custom(Message::Flush)])
}

/// A text editor without frame (Linear's title and description): text in `value`, placeholder
/// `text_3`. With `surface` it shows the hover surface while pointed at or typed in (the notes),
/// else focus shows only as the caret (the title).
fn bare_editor(theme: &Theme, status: text_editor::Status, value: Color, surface: bool) -> text_editor::Style {
    let p = palette(theme);
    let base = style::text_editor(theme, status);
    let faded = matches!(status, text_editor::Status::Disabled);
    let lit = surface && matches!(status, text_editor::Status::Hovered | text_editor::Status::Focused { .. });
    text_editor::Style {
        background: Background::Color(if lit { p.hover } else { Color::TRANSPARENT }),
        border: border::rounded(theme::radius::SM).width(0).color(Color::TRANSPARENT),
        placeholder: p.text_3,
        value: if faded { value.scale_alpha(style::DISABLED_ALPHA) } else { value },
        ..base
    }
}

/// A text input that looks like a row ("Add sub-task"): no frame, the hover surface while
/// pointed at or typed in, placeholder `text_3`.
fn bare_input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = palette(theme);
    let base = style::text_input(theme, status);
    let lit = matches!(status, text_input::Status::Hovered | text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(if lit { p.hover } else { Color::TRANSPARENT }),
        border: border::rounded(theme::radius::SM).width(0).color(Color::TRANSPARENT),
        placeholder: p.text_3,
        value: p.text,
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
        // Not edited here: saved as loaded, so the server keeps them.
        ..original.clone()
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
    use torro_pim::CollectionKind;
    use torro_pim::model::GitHubLabel;

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
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
        }
    }

    fn list(id: &str, name: &str) -> Collection {
        Collection {
            id: id.into(),
            name: name.into(),
            kind: CollectionKind::Todos,
            color: None,
            writable: true,
            source: None,
        }
    }

    fn lists() -> Vec<Collection> {
        vec![list("work", "Work"), list("home", "Home")]
    }

    /// The details GitHub reports for a pull request: open, one label, an assignee, a
    /// milestone, checks running and a review asked for.
    fn pull_request() -> GitHubDetails {
        GitHubDetails {
            kind: GitHubKind::PullRequest,
            state: GitHubState::Open,
            labels: vec![GitHubLabel { name: "ui".into(), color: Some("#3B82F6".into()) }],
            assignees: vec!["anna".into()],
            milestone: Some("v0.4".into()),
            milestone_due: None,
            draft: false,
            review: Some(ReviewState::Required),
            checks: Some(ChecksState::Pending),
            conflict: false,
        }
    }

    /// A GitHub item as a GitHub filter list delivers it: in a read-only list, with details.
    fn github_item(details: GitHubDetails) -> Todo {
        let link = Link { github: Some(details), ..Link::github("mahype/TorroWorkplace", 131) };
        Todo {
            collection: "github:mahype/needs-me".into(),
            url: Some("https://github.com/mahype/TorroWorkplace/pull/131".into()),
            link: Some(link),
            ..todo("p131", "Add workspace templates to the tray menu")
        }
    }

    /// The detail of `task`, with `others` next to it and today fixed to Sunday 11 October 2026.
    fn detail(task: &Todo, others: &[Todo]) -> Detail {
        let mut all = others.to_vec();
        all.push(task.clone());
        let mut detail = Detail::new(task, &lists(), &all);
        detail.today = day(11);
        detail
    }

    /// [`detail`] without other tasks, for tests that name their detail `detail`.
    fn detail_of(task: &Todo) -> Detail {
        detail(task, &[])
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

    /// The rows of the properties column, flat, with the title of their block.
    fn rows(detail: &Detail, account: bool) -> Vec<(Option<&'static str>, Prop)> {
        detail
            .property_blocks(account)
            .into_iter()
            .flat_map(|(title, props)| props.into_iter().map(move |prop| (title, prop)))
            .collect()
    }

    fn props(detail: &Detail) -> Vec<Prop> {
        rows(detail, false).into_iter().map(|(_, prop)| prop).collect()
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
    fn a_new_subtask_goes_into_the_list_below_this_task() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert_eq!(detail.update(Message::SubtaskSubmit), Action::None, "nothing typed");
        detail.update(Message::SubtaskEdited("  Read the concept ".into()));
        match detail.update(Message::SubtaskSubmit) {
            Action::Create(new) => {
                assert_eq!(new.summary, "Read the concept");
                assert_eq!(new.parent.as_deref(), Some("t1"));
                assert_eq!(new.collection, "work");
                assert!(new.uid.is_empty() && new.href.is_none() && new.etag.is_none(), "a new task");
            }
            other => panic!("expected a new sub-task, got {other:?}"),
        }
        assert!(detail.new_subtask.is_empty(), "the field empties for the next one");
    }

    #[test]
    fn hide_completed_and_folding_toggle() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        detail.update(Message::ToggleHideDone);
        assert!(detail.hide_done);
        detail.update(Message::Fold(Section::Subtasks));
        assert!(detail.folded.contains(&Section::Subtasks));
        detail.update(Message::Fold(Section::Subtasks));
        assert!(!detail.folded.contains(&Section::Subtasks));
    }

    #[test]
    fn delete_in_the_menu_asks_once_before_deleting() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        detail.update(Message::Open(Editor::More));
        assert_eq!(detail.update(Message::Delete), Action::None);
        assert!(detail.confirm_delete && detail.open == Some(Editor::More), "the menu asks");
        assert_eq!(detail.update(Message::CancelDelete), Action::None);
        assert!(!detail.confirm_delete && detail.open.is_none());

        detail.update(Message::Open(Editor::More));
        detail.update(Message::Delete);
        match detail.update(Message::ConfirmDelete) {
            Action::Delete(todo) => assert_eq!(todo.uid, "t1"),
            other => panic!("expected a delete, got {other:?}"),
        }
        assert!(!detail.confirm_delete && detail.open.is_none());
    }

    #[test]
    fn escape_closes_the_menu_and_its_question_then_the_detail() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert_eq!(detail.update(Message::Open(Editor::Due)), Action::Focus(DUE_INPUT));
        assert_eq!(detail.update(Message::Escape), Action::None);
        assert_eq!(detail.open, None);
        detail.update(Message::Open(Editor::More));
        detail.update(Message::Delete);
        assert_eq!(detail.update(Message::Escape), Action::None);
        assert!(!detail.confirm_delete && detail.open.is_none());
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
    fn the_arrows_in_the_head_ask_for_the_neighbours() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        detail.update(Message::Open(Editor::More));
        assert_eq!(detail.update(Message::Previous), Action::Previous);
        assert_eq!(detail.open, None, "the menu closes");
        assert_eq!(detail.update(Message::Next), Action::Next);
    }

    #[test]
    fn the_more_menu_offers_what_the_task_has() {
        assert_eq!(detail_of(&todo("t1", "a")).more_items(), [More::Delete], "a plain task");
        let linked = Todo { url: Some("https://example.org/a".into()), ..todo("t1", "a") };
        assert_eq!(detail_of(&linked).more_items(), [More::CopyLink, More::OpenLink, More::Delete]);
        let read_only = Todo { collection: "elsewhere".into(), ..todo("t1", "a") };
        assert!(detail_of(&read_only).more_items().is_empty(), "nothing to do: no \"…\"");
    }

    #[test]
    fn the_more_menu_never_deletes_a_github_item() {
        let item = github_item(pull_request());
        let mut detail = detail_of(&item);
        assert_eq!(detail.more_items(), [More::CopyLink, More::OpenLink]);
        detail.update(Message::Open(Editor::More));
        detail.update(Message::Delete);
        assert!(!detail.confirm_delete);
        assert_eq!(detail.update(Message::ConfirmDelete), Action::None);
        // Also not when it sits in a writable list.
        let in_list = Todo { collection: "work".into(), ..item };
        assert!(!detail_of(&in_list).more_items().contains(&More::Delete));
    }

    #[test]
    fn copy_link_copies_the_web_address_and_closes_the_menu() {
        let mut detail = detail_of(&github_item(pull_request()));
        detail.update(Message::Open(Editor::More));
        let page = "https://github.com/mahype/TorroWorkplace/pull/131";
        assert_eq!(detail.update(Message::CopyLink), Action::CopyLink(page.into()));
        assert_eq!(detail.open, None);
    }

    #[test]
    fn properties_of_a_plain_task_offer_what_can_be_set() {
        let detail = detail_of(&todo("t1", "a"));
        let organisation = Some(tr("Organisation"));
        assert_eq!(
            rows(&detail, true),
            [
                (None, Prop::Status),
                (None, Prop::Priority),
                (None, Prop::Due),
                (organisation, Prop::List),
                (organisation, Prop::Parent),
                (Some(tr("Source")), Prop::Account),
            ]
        );
        assert_eq!(detail.status_word(), tr("Open"));
        assert_eq!((detail.draft.priority, detail.due_label(), detail.parent_title()), (Priority::None, None, None));
        assert!(rows(&detail, false).iter().all(|(title, _)| *title != Some(tr("Source"))), "no empty Source block");
    }

    #[test]
    fn properties_of_a_task_show_what_is_set() {
        let task = Todo {
            completed: true,
            priority: Some(5),
            due: Some(EventTime::Date(day(9))),
            parent: Some("p".into()),
            categories: vec!["api".into(), " ".into()],
            url: Some("https://example.org/a".into()),
            ..todo("t1", "a")
        };
        let detail = detail(&task, &[todo("p", "Plan the API release")]);
        let props = props(&detail);
        assert!(props.contains(&Prop::Labels) && props.contains(&Prop::OpenLink));
        assert!(!props.contains(&Prop::Repo), "no GitHub rows for a task");
        assert_eq!(detail.status_word(), tr("Completed"));
        assert_eq!(detail.labels(), [(String::new(), "api".to_string())]);
        assert_eq!(detail.parent_title().as_deref(), Some("Plan the API release"));
        assert!(!detail.overdue(), "a done task is not overdue");
        assert_eq!(detail.chip_props(), [Prop::Status, Prop::Priority, Prop::Due, Prop::List]);
    }

    #[test]
    fn properties_of_a_github_item_show_its_source() {
        let detail = detail_of(&github_item(pull_request()));
        let (organisation, source) = (Some(tr("Organisation")), Some(tr("Source")));
        assert_eq!(
            rows(&detail, true),
            [
                (None, Prop::Status),
                (organisation, Prop::List),
                (organisation, Prop::Labels),
                (source, Prop::Account),
                (source, Prop::Repo),
                (source, Prop::Assignees),
                (source, Prop::Milestone),
                (source, Prop::OpenLink),
            ],
            "read-only: no placeholder rows for priority, due or parent"
        );
        assert_eq!(detail.status_word(), tr("Open"));
        assert_eq!(detail.labels(), [("#3B82F6".to_string(), "ui".to_string())]);
        assert_eq!(detail.place_name(), "TorroWorkplace", "the repository stands in for the list");
        assert_eq!(detail.place_mark(), Mark::Icon(Icon::Github));
        assert_eq!(detail.chip_props(), [Prop::List], "the state is a pill, nothing else is set");
    }

    #[test]
    fn a_github_item_in_a_task_list_can_still_be_planned() {
        let item =
            Todo { collection: "work".into(), due: Some(EventTime::Date(day(10))), ..github_item(pull_request()) };
        let detail = detail_of(&item);
        let props = props(&detail);
        assert!(props.contains(&Prop::Priority) && props.contains(&Prop::Parent));
        assert!(detail.overdue());
        assert_eq!(detail.place_name(), "Work");
        assert_eq!(detail.chip_props(), [Prop::Priority, Prop::Due, Prop::List]);
    }

    #[test]
    fn github_states_have_words_and_icons() {
        let pr = |state, draft| GitHubDetails { state, draft, ..pull_request() };
        let issue = |state| GitHubDetails { kind: GitHubKind::Issue, ..pr(state, false) };
        let cases = [
            (pr(GitHubState::Open, false), Icon::GitPullRequest, tr("Open")),
            (pr(GitHubState::Open, true), Icon::GitPullRequestDraft, tr("Draft")),
            (pr(GitHubState::Merged, false), Icon::GitMerge, tr("Merged")),
            (pr(GitHubState::Closed, false), Icon::GitPullRequestClosed, tr("Closed")),
            (issue(GitHubState::Open), Icon::CircleDot, tr("Open")),
            (issue(GitHubState::Closed), Icon::CircleCheck, tr("Closed")),
        ];
        for (details, icon, word) in cases {
            let look = state_look(&details);
            assert_eq!((look.icon, look.word), (icon, word), "{details:?}");
        }
        assert_eq!(checks_look(ChecksState::Failure).word, tr("Checks failed"));
        assert_eq!(review_look(ReviewState::Approved).word, tr("Approved"));
    }

    #[test]
    fn checks_and_reviews_only_for_pull_requests_that_have_them() {
        let pr = detail_of(&github_item(pull_request()));
        assert!(pr.checks_section().is_some() && pr.reviews_section().is_some());
        let quiet = detail_of(&github_item(GitHubDetails { review: None, checks: None, ..pull_request() }));
        assert!(quiet.checks_section().is_none() && quiet.reviews_section().is_none());
        let issue = detail_of(&github_item(GitHubDetails { kind: GitHubKind::Issue, ..pull_request() }));
        assert!(issue.checks_section().is_none() && issue.reviews_section().is_none());
        let task = detail_of(&todo("t1", "a"));
        assert!(task.checks_section().is_none() && task.subtasks_section().is_some(), "Add sub-task is there");
        assert!(pr.subtasks_section().is_none(), "a read-only item without sub-tasks has none");
    }

    #[test]
    fn every_view_builds_in_every_fit() {
        let child = Todo { parent: Some("t1".into()), completed: true, ..todo("c", "Child") };
        let anchor = Anchor::new();
        for task in [todo("t1", "a"), github_item(pull_request())] {
            let mut detail = detail(&task, std::slice::from_ref(&child));
            detail.update(Message::Open(Editor::More));
            for fit in [Fit::Columns, Fit::Chips, Fit::Narrow] {
                let _ = detail.head(fit, "Today".into(), &anchor);
                let _ = detail.main_column(fit, Palette::light(), &anchor);
            }
            let _ = detail.properties_column(Some("anna@cloud.example.org".into()), &anchor);
            let _ = detail.property_chips(Palette::dark(), &anchor);
            assert!(detail.layer(&anchor).is_some(), "the \"…\" menu floats");
        }
    }

    #[test]
    fn open_link_opens_the_url_else_the_link_page() {
        let own = Todo {
            url: Some(" https://example.org/a ".into()),
            link: Some(Link::github("mahype/TorroWorkplace", 123)),
            ..todo("t1", "a")
        };
        let mut detail = detail(&own, &[]);
        assert_eq!(detail.update(Message::OpenLink), Action::OpenUrl("https://example.org/a".into()));

        let linked = Todo { link: Some(Link::github("mahype/TorroWorkplace", 123)), ..todo("t2", "b") };
        let mut detail = detail_of(&linked);
        let page = "https://github.com/mahype/TorroWorkplace/issues/123";
        assert_eq!(detail.update(Message::OpenLink), Action::OpenUrl(page.into()));
        let props = props(&detail);
        assert!(props.contains(&Prop::Repo) && props.contains(&Prop::OpenLink), "a linked task names its repo");
    }

    #[test]
    fn only_web_addresses_are_offered() {
        for url in ["file:///etc/passwd", "javascript:alert(1)", "zoommtg://x", ""] {
            let other = Link { id: "jira:ABC-1".into(), github: None };
            let task = Todo { url: Some(url.into()), link: Some(other), ..todo("t1", "a") };
            let mut detail = detail_of(&task);
            assert_eq!(detail.link_target(), None, "{url}");
            assert!(!props(&detail).contains(&Prop::OpenLink), "{url}");
            assert!(!detail.more_items().contains(&More::CopyLink), "{url}");
            assert_eq!(detail.update(Message::OpenLink), Action::None, "{url}");
        }
    }

    #[test]
    fn tags_link_and_order_survive_a_save() {
        let task = Todo {
            categories: vec!["Finance".into(), " ".into()],
            url: Some("https://example.org".into()),
            sort_order: Some(3),
            link: Some(Link::github("mahype/TorroWorkplace", 1)),
            ..todo("t1", "a")
        };
        let mut detail = detail_of(&task);
        let saved = saves(&mut detail, Message::ToggleDone);
        assert_eq!(
            (saved.categories, saved.url, saved.sort_order, saved.link),
            (task.categories, task.url, task.sort_order, task.link),
            "the detail does not edit them, so they are saved as loaded"
        );
    }

    #[test]
    fn a_read_only_task_takes_no_sub_tasks() {
        let read_only = Todo { collection: "github:mahype/needs-me".into(), ..todo("t1", "a") };
        let mut detail = detail_of(&read_only);
        assert!(!detail.editable());
        detail.update(Message::SubtaskEdited("x".into()));
        assert_eq!(detail.update(Message::SubtaskSubmit), Action::None);
    }

    #[test]
    fn close_closes() {
        let mut detail = detail(&todo("t1", "a"), &[]);
        assert_eq!(detail.update(Message::Close), Action::Close);
    }
}
