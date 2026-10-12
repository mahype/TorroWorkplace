//! The dialog of a GitHub filter list (task G.4 in `docs/tasks/github.md`, concept "Filterliste" in
//! `docs/github.md`): a new filter list ("New list…" › "GitHub filter list", `new_list.rs`), "Edit
//! filter…" and the question before "Delete list…".
//!
//! **The search is the truth.** A filter list is saved GitHub searches; everything in the dialog
//! only helps to write them. On top a field takes a pasted search or the address of an issue list
//! on github.com (the core turns addresses into searches when it previews or saves). Below it the
//! form – type, state, repositories, labels with and without, assignee, author, mentioned, review
//! requested, milestone, sort – writes the search into the field under it, which can be edited by
//! hand. The form reads and rewrites only the words it knows ([`Field`], [`Kind`], [`State`],
//! [`Sort`]), in place; every other word of the search stays exactly where it was. Text fields
//! keep what was typed (a trailing ", " included), so typing never fights the rewriting.
//!
//! A list may run up to [`MAX_SEARCHES`] searches; it holds what any of them finds. Templates
//! (Assigned to me, Review requested, My open pull requests, Mentioned) replace the search being
//! edited and name an unnamed list.
//!
//! **Preview:** "{n} matches" and the first matches come from the core (`tasks.preview_filter`),
//! which asks GitHub once per search – so not on every key: a change waits [`DEBOUNCE`] for the
//! next one, and only the newest answer counts.
//!
//! **Saving** sends `tasks.create_filter_list` or `tasks.update_filter_list` with the revision the
//! dialog was opened with. When someone changed the list meanwhile the core answers `conflict`:
//! the dialog says so, the app reloads, and the dialog shows the current version
//! ([`Dialog::reloaded`]). Errors of the form show after the first try to save.
//!
//! Wide and medium windows show the dialog over the window (like the control centre's account
//! assistant), narrow windows as a view of its own over the whole window. "Delete list…" asks
//! first ("The issues stay on GitHub.").
//!
//! The app holds an `Option<Dialog>`, sends its [`Message`]s to [`Dialog::update`] and acts on the
//! [`Outcome`]; requests go out through the app's [`Source`].

use std::fmt;
use std::time::Duration;

use iced::time::Instant;
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Space, button, column, container, operation, pick_list, row, rule, scrollable, text, text_input,
};
use iced::{Alignment, Color, Element, Fill, Padding, Size, Subscription, Task, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::source::Source;
use torro_gui::theme::{self, Palette, palette, size, space};
use torro_gui::widgets::{self, Mark, collection_color};
use torro_gui::{style, typo};
use torro_pim::source::{FilterDefinition, FilterPreview};
use torro_pim::{Collection, PimSource, ProviderError, Todo};

use crate::list::{self, Tone};

/// A list runs at most this many searches (the core's limit).
pub const MAX_SEARCHES: usize = 10;
/// How long the preview waits after a change for the next one before it asks the core.
pub const DEBOUNCE: Duration = Duration::from_millis(500);
/// How often the dialog looks whether the wait is over, while (and only while) a preview waits.
const TICK: Duration = Duration::from_millis(100);
/// How many of the first matches the preview lists.
const PREVIEW_ROWS: usize = 5;
/// The dialog is at most as high as it is wide plus its head and foot.
const MAX_HEIGHT: f32 = size::FORM_MAX + 2.0 * size::HEADER;
/// Width of the line between the columns.
const LINE: f32 = size::HAIRLINE;

/// Id of the name field: focused when a new list opens and when the name is missing on save.
pub const NAME_INPUT: &str = "filter-name";
/// Id of the search field: focused when no search is there on save.
const QUERY_INPUT: &str = "filter-query";

// ---------------------------------------------------------------------------------------------
// The words of a search
// ---------------------------------------------------------------------------------------------

/// The words of `query` as written: split at white space outside double quotes, so
/// `label:"good first issue"` stays one word.
pub fn words(query: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    for c in query.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                word.push(c);
            }
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// One word read: `-label:"needs info"` is negated, qualifier `label`, value `needs info`. A word
/// without a qualifier (free text, `OR`) has none.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Word {
    negated: bool,
    /// Lower case; GitHub takes qualifiers in any case.
    qualifier: Option<String>,
    /// Without the quotes.
    value: String,
}

fn read(word: &str) -> Word {
    let (negated, rest) = match word.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, word),
    };
    match rest.split_once(':') {
        Some((qualifier, value)) if !qualifier.is_empty() && !qualifier.contains('"') => {
            Word { negated, qualifier: Some(qualifier.to_lowercase()), value: value.trim_matches('"').to_string() }
        }
        _ => Word { negated: false, qualifier: None, value: word.to_string() },
    }
}

/// `qualifier:value` (`-` in front when `negated`), the value quoted when it has a space.
fn write(qualifier: &str, negated: bool, value: &str) -> String {
    let value = value.replace('"', "");
    let minus = if negated { "-" } else { "" };
    if value.chars().any(char::is_whitespace) {
        format!("{minus}{qualifier}:\"{value}\"")
    } else {
        format!("{minus}{qualifier}:{value}")
    }
}

/// `query` with the words at `owned` (ascending) replaced by `new`, which go where the first of
/// them was (at the end when there was none). Every other word keeps its place.
fn replace(query: &str, owned: &[usize], new: Vec<String>) -> String {
    let mut all = words(query);
    let at = owned.first().copied().unwrap_or(all.len());
    for &index in owned.iter().rev() {
        all.remove(index);
    }
    let tail = all.split_off(at.min(all.len()));
    all.extend(new);
    all.extend(tail);
    all.join(" ")
}

/// Whether `query` is the address of a page on github.com rather than a search. The core turns it
/// into a search; the form cannot read it.
pub fn is_address(query: &str) -> bool {
    let lower = query.trim().to_lowercase();
    ["https://", "http://", "github.com/", "www.github.com/"].iter().any(|start| lower.starts_with(start))
}

// ---------------------------------------------------------------------------------------------
// The form: fields and choices that read and write words of the search
// ---------------------------------------------------------------------------------------------

/// A text field of the form and the qualifier it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    /// `repo:owner/name`, several (any of them), separated by commas or spaces.
    Repos,
    /// `label:name`, several (all of them), separated by commas.
    Labels,
    /// `-label:name`, several, separated by commas.
    WithoutLabels,
    /// `assignee:login` (`@me`).
    Assignee,
    /// `author:login`.
    Author,
    /// `mentions:login`.
    Mentions,
    /// `review-requested:login`.
    ReviewRequested,
    /// `milestone:"name"`.
    Milestone,
}

impl Field {
    pub const ALL: [Field; 8] = [
        Field::Repos,
        Field::Labels,
        Field::WithoutLabels,
        Field::Assignee,
        Field::Author,
        Field::Mentions,
        Field::ReviewRequested,
        Field::Milestone,
    ];

    fn index(self) -> usize {
        Field::ALL.iter().position(|&field| field == self).unwrap_or_default()
    }

    fn qualifier(self) -> &'static str {
        match self {
            Field::Repos => "repo",
            Field::Labels | Field::WithoutLabels => "label",
            Field::Assignee => "assignee",
            Field::Author => "author",
            Field::Mentions => "mentions",
            Field::ReviewRequested => "review-requested",
            Field::Milestone => "milestone",
        }
    }

    fn negated(self) -> bool {
        self == Field::WithoutLabels
    }

    /// Takes several values; the others take one.
    fn many(self) -> bool {
        matches!(self, Field::Repos | Field::Labels | Field::WithoutLabels)
    }

    /// Whether `word` is one of this field's. `label:a,b` (any of two) is not: the field joins its
    /// labels with commas and would read it as two labels that must both be there.
    fn takes(self, word: &Word) -> bool {
        word.qualifier.as_deref() == Some(self.qualifier())
            && word.negated == self.negated()
            && !(matches!(self, Field::Labels | Field::WithoutLabels) && word.value.contains(','))
    }

    /// The indices of the words of `query` this field reads: all of its kind, or the first.
    fn owned(self, query: &str) -> Vec<usize> {
        let found = words(query).into_iter().enumerate().filter(|(_, w)| self.takes(&read(w))).map(|(i, _)| i);
        if self.many() { found.collect() } else { found.take(1).collect() }
    }

    /// The values typed into the field: split at commas (repositories also at spaces), trimmed.
    fn values(self, text: &str) -> Vec<String> {
        let split: Vec<&str> = match self {
            Field::Repos => text.split(|c: char| c == ',' || c.is_whitespace()).collect(),
            Field::Labels | Field::WithoutLabels => text.split(',').collect(),
            _ => vec![text],
        };
        split.into_iter().map(|v| v.trim().replace('"', "")).filter(|v| !v.is_empty()).collect()
    }

    pub fn label(self) -> &'static str {
        match self {
            Field::Repos => tr("Repositories"),
            Field::Labels => tr("Labels"),
            Field::WithoutLabels => tr("Without labels"),
            Field::Assignee => tr("Assignee"),
            Field::Author => tr("Author"),
            Field::Mentions => tr("Mentions"),
            Field::ReviewRequested => tr("Review requested"),
            Field::Milestone => tr("Milestone"),
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Field::Repos => tr("owner/name, owner/name"),
            Field::Labels => tr("bug, good first issue"),
            Field::WithoutLabels => tr("wontfix"),
            Field::Assignee | Field::Author | Field::Mentions | Field::ReviewRequested => tr("@me or a login"),
            Field::Milestone => tr("Milestone name"),
        }
    }
}

/// The values of the words of `query` that `field` reads.
fn field_values(query: &str, field: Field) -> Vec<String> {
    let all = words(query);
    field.owned(query).iter().map(|&i| read(&all[i]).value).collect()
}

/// What `field` reads from `query`, as the field shows it: values joined by ", ".
pub fn field_text(query: &str, field: Field) -> String {
    field_values(query, field).join(", ")
}

/// `query` with the words of `field` set from `text` (as typed into the field), in place of the
/// old ones; unchanged when the values are the same, so writing back what was read never changes
/// the search. An empty `text` removes the field's words.
pub fn set_field(query: &str, field: Field, text: &str) -> String {
    let new = field.values(text);
    if new == field_values(query, field) {
        return query.to_string();
    }
    let new = new.iter().map(|value| write(field.qualifier(), field.negated(), value)).collect();
    replace(query, &field.owned(query), new)
}

/// A choice of the form: which positive word it reads and which it writes.
trait Choice: Sized + PartialEq + Clone {
    /// The choice `word` stands for, if it is one of this kind.
    fn from_word(word: &Word) -> Option<Self>;
    /// The word for this choice; `None` for the one GitHub takes without a word.
    fn word(&self) -> Option<String>;
    /// The choice without a word.
    fn unset() -> Self;

    /// The choice in `query` and where its word is: the first positive word of this kind.
    fn find(query: &str) -> (Self, Option<usize>) {
        words(query)
            .iter()
            .enumerate()
            .find_map(|(i, w)| {
                Some(read(w)).filter(|w| !w.negated).and_then(|w| Self::from_word(&w)).map(|c| (c, Some(i)))
            })
            .unwrap_or((Self::unset(), None))
    }

    fn of(query: &str) -> Self {
        Self::find(query).0
    }

    /// `query` with this choice in place of the old one; unchanged when it is the same.
    fn put(&self, query: &str) -> String {
        let (old, at) = Self::find(query);
        if old == *self {
            return query.to_string();
        }
        replace(query, &at.into_iter().collect::<Vec<_>>(), self.word().into_iter().collect())
    }
}

/// Issues, pull requests or both (`is:issue`, `is:pr`; `type:` is read as well).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Issues,
    PullRequests,
    Both,
}

impl Choice for Kind {
    fn from_word(word: &Word) -> Option<Kind> {
        match (word.qualifier.as_deref()?, word.value.to_lowercase().as_str()) {
            ("is" | "type", "issue") => Some(Kind::Issues),
            ("is" | "type", "pr") => Some(Kind::PullRequests),
            _ => None,
        }
    }

    fn word(&self) -> Option<String> {
        match self {
            Kind::Issues => Some("is:issue".into()),
            Kind::PullRequests => Some("is:pr".into()),
            Kind::Both => None,
        }
    }

    fn unset() -> Kind {
        Kind::Both
    }
}

/// Open, closed or any (`is:open`, `is:closed`; `state:` is read as well).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Open,
    Closed,
    Any,
}

impl Choice for State {
    fn from_word(word: &Word) -> Option<State> {
        match (word.qualifier.as_deref()?, word.value.to_lowercase().as_str()) {
            ("is" | "state", "open") => Some(State::Open),
            ("is" | "state", "closed") => Some(State::Closed),
            _ => None,
        }
    }

    fn word(&self) -> Option<String> {
        match self {
            State::Open => Some("is:open".into()),
            State::Closed => Some("is:closed".into()),
            State::Any => None,
        }
    }

    fn unset() -> State {
        State::Any
    }
}

/// The order of the matches (`sort:…`); a list shows its items in this order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sort {
    /// No `sort:` word: GitHub's best match.
    BestMatch,
    /// `sort:created-desc`.
    Newest,
    /// `sort:created-asc`.
    Oldest,
    /// `sort:updated-desc`.
    Updated,
    /// `sort:comments-desc`.
    Comments,
    /// Any other `sort:` value, kept as written.
    Other(String),
}

impl Sort {
    /// The choices of the sort menu; `current` among them when it is another value.
    fn choices(current: &Sort) -> Vec<Sort> {
        let mut all = vec![Sort::BestMatch, Sort::Newest, Sort::Oldest, Sort::Updated, Sort::Comments];
        if let Sort::Other(_) = current {
            all.push(current.clone());
        }
        all
    }
}

impl fmt::Display for Sort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Sort::BestMatch => f.write_str(tr("Best match")),
            Sort::Newest => f.write_str(tr("Newest")),
            Sort::Oldest => f.write_str(tr("Oldest")),
            Sort::Updated => f.write_str(tr("Recently updated")),
            Sort::Comments => f.write_str(tr("Most commented")),
            Sort::Other(value) => write!(f, "sort:{value}"),
        }
    }
}

impl Choice for Sort {
    fn from_word(word: &Word) -> Option<Sort> {
        if word.qualifier.as_deref()? != "sort" {
            return None;
        }
        Some(match word.value.as_str() {
            "created-desc" => Sort::Newest,
            "created-asc" => Sort::Oldest,
            "updated-desc" => Sort::Updated,
            "comments-desc" => Sort::Comments,
            other => Sort::Other(other.to_string()),
        })
    }

    fn word(&self) -> Option<String> {
        let value = match self {
            Sort::BestMatch => return None,
            Sort::Newest => "created-desc",
            Sort::Oldest => "created-asc",
            Sort::Updated => "updated-desc",
            Sort::Comments => "comments-desc",
            Sort::Other(value) => value,
        };
        Some(write("sort", false, value))
    }

    fn unset() -> Sort {
        Sort::BestMatch
    }
}

/// A search to start from (`docs/github.md`, "Vorlagen").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    AssignedToMe,
    ReviewRequested,
    MyPullRequests,
    Mentioned,
}

impl Template {
    pub const ALL: [Template; 4] =
        [Template::AssignedToMe, Template::ReviewRequested, Template::MyPullRequests, Template::Mentioned];

    pub fn query(self) -> &'static str {
        match self {
            Template::AssignedToMe => "is:open assignee:@me",
            Template::ReviewRequested => "is:open is:pr review-requested:@me",
            Template::MyPullRequests => "is:open is:pr author:@me",
            Template::Mentioned => "is:open mentions:@me",
        }
    }

    /// The template's name, which an unnamed list takes.
    pub fn label(self) -> &'static str {
        match self {
            Template::AssignedToMe => tr("Assigned to me"),
            Template::ReviewRequested => tr("Review requested"),
            Template::MyPullRequests => tr("My open pull requests"),
            Template::Mentioned => tr("Mentioned"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Template::AssignedToMe => Icon::UserCheck,
            Template::ReviewRequested => Icon::Eye,
            Template::MyPullRequests => Icon::GitPullRequest,
            Template::Mentioned => Icon::User,
        }
    }
}

/// One search being edited: the search itself and what the form's text fields show.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Search {
    query: String,
    /// The text of each [`Field`] as typed, by [`Field::index`].
    texts: [String; Field::ALL.len()],
}

impl Search {
    fn new(query: &str) -> Search {
        Search { query: query.trim().to_string(), texts: Field::ALL.map(|field| field_text(query, field)) }
    }

    fn text(&self, field: Field) -> &str {
        &self.texts[field.index()]
    }

    fn set_text(&mut self, field: Field, text: String) {
        self.query = set_field(&self.query, field, &text);
        self.texts[field.index()] = text;
    }
}

// ---------------------------------------------------------------------------------------------
// The dialog
// ---------------------------------------------------------------------------------------------

/// How the app opens the dialog (sidebar, the list's "…" menu, `--open`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Open {
    /// A new filter list of the GitHub account with this id ("New list…" › "GitHub filter list").
    New(String),
    /// "Edit filter…" of the filter list with this collection id.
    Edit(String),
    /// "Delete list…" of the filter list with this collection id.
    Delete(String),
}

/// What [`Dialog::update`] asks the app to do besides running the returned task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Keep the dialog open.
    Stay,
    /// Close the dialog; nothing changed.
    Close,
    /// The list with this id was created or changed: close, show it, reload.
    Saved(String),
    /// The list with this id was deleted: close, leave it if it is shown, reload.
    Deleted(String),
    /// Reload the collections and hand them to [`Dialog::reloaded`] (after a conflict).
    Reload,
}

#[derive(Debug, Clone)]
pub enum Message {
    Name(String),
    Color(Option<String>),
    Template(Template),
    /// Edit the search at this index.
    Select(usize),
    AddSearch,
    RemoveSearch(usize),
    /// Typed into the field on top.
    Paste(String),
    /// Enter in the field on top, or something pasted into it: it becomes the search.
    Pasted(String),
    Kind(Kind),
    State(State),
    Sort(Sort),
    Text(Field, String),
    /// The search typed by hand.
    Query(String),
    /// While a preview waits: is the wait over?
    Tick(Instant),
    /// The core's answer to the preview asked for as the given one.
    Previewed(u64, Result<FilterPreview, String>),
    Save,
    /// The core's answer to Save: the id of the list as stored.
    Saved(Result<String, ProviderError>),
    /// Cancel, the close button and Escape.
    Cancel,
    /// A click beside the dialog: nothing happens. A half-made list is too much work to lose by
    /// a stray click; Cancel and Escape close it.
    Backdrop,
    /// "Delete list" in the question.
    ConfirmDelete,
    Deleted(Result<(), String>),
}

/// The open dialog: the editor of a list, or the question before deleting one.
#[derive(Debug, Clone)]
pub enum Dialog {
    Edit(Box<Editor>),
    Delete(Delete),
}

/// Where a save goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    New,
    Edit { collection: String, revision: u64 },
}

/// What the editor says above the form.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Notice {
    /// Saving failed; the reason.
    Failed(String),
    /// The core answered `conflict`; the current version is being loaded.
    Conflict,
    /// The current version was loaded after a conflict.
    Reloaded,
}

/// "New filter list…" and "Edit filter…".
#[derive(Debug, Clone)]
pub struct Editor {
    target: Target,
    /// The GitHub account, `github:<login>`: where the preview runs and a new list goes.
    account: String,
    /// The account as the sidebar names it ("GitHub · mahype").
    account_label: String,
    name: String,
    color: Option<String>,
    /// One to [`MAX_SEARCHES`].
    searches: Vec<Search>,
    current: usize,
    /// The field on top, until its text becomes the search.
    paste: String,
    /// The last answer of the core, kept while a newer one is asked for.
    preview: Option<Result<FilterPreview, String>>,
    /// When the searches last changed while no preview has been asked for them since.
    changed: Option<Instant>,
    /// The number of the newest preview asked for; older answers are dropped.
    asked: u64,
    loading: bool,
    /// Save was pressed once: the form shows its errors from now on.
    tried: bool,
    saving: bool,
    notice: Option<Notice>,
}

/// The question before "Delete list…".
#[derive(Debug, Clone)]
pub struct Delete {
    collection: String,
    name: String,
    busy: bool,
    problem: Option<String>,
}

impl Dialog {
    /// "New filter list…" of the GitHub account `account` (`github:<login>`), shown as
    /// `account_label`. Starts with one empty search; nothing is asked of the core yet.
    pub fn new_list(account: String, account_label: String) -> Dialog {
        Dialog::Edit(Box::new(Editor {
            target: Target::New,
            account,
            account_label,
            name: String::new(),
            color: None,
            searches: vec![Search::new("")],
            current: 0,
            paste: String::new(),
            preview: None,
            changed: None,
            asked: 0,
            loading: false,
            tried: false,
            saving: false,
            notice: None,
        }))
    }

    /// "Edit filter…" of `list`, a filter list of `account` (shown as `account_label`); `None` for
    /// every other collection. Its preview follows at once, after [`DEBOUNCE`].
    pub fn edit(list: &Collection, account: String, account_label: String) -> Option<Dialog> {
        let source = list.source.as_ref()?;
        let mut dialog = Dialog::new_list(account, account_label);
        if let Dialog::Edit(editor) = &mut dialog {
            editor.target = Target::Edit { collection: list.id.clone(), revision: source.revision };
            editor.load(list);
            editor.touch(Instant::now());
        }
        Some(dialog)
    }

    /// The question before deleting `list`; `None` for a collection that is no filter list.
    pub fn delete(list: &Collection) -> Option<Dialog> {
        list.source.as_ref()?;
        Some(Dialog::Delete(Delete {
            collection: list.id.clone(),
            name: list.name.clone(),
            busy: false,
            problem: None,
        }))
    }

    /// Carries out `message`; requests go out through `source`. The task is the app's to run.
    pub fn update(&mut self, message: Message, source: &Source) -> (Task<Message>, Outcome) {
        match self {
            Dialog::Edit(editor) => editor.update(message, source),
            Dialog::Delete(delete) => delete.update(message, source),
        }
    }

    /// While a preview waits for the typing to stop: a tick every [`TICK`]; nothing otherwise.
    pub fn subscription(&self) -> Subscription<Message> {
        match self {
            Dialog::Edit(editor) if editor.changed.is_some() => iced::time::every(TICK).map(Message::Tick),
            _ => Subscription::none(),
        }
    }

    /// The collections after a reload. After a conflict the editor takes the list's current
    /// version from them (name, colour, searches, revision) and says so; otherwise nothing
    /// changes – a reload every minute must not undo what is being typed.
    pub fn reloaded(&mut self, collections: &[Collection]) {
        let Dialog::Edit(editor) = self else { return };
        let Target::Edit { collection, .. } = &editor.target else { return };
        if editor.notice != Some(Notice::Conflict) {
            return;
        }
        match collections.iter().find(|c| &c.id == collection && c.source.is_some()) {
            Some(list) => {
                editor.load(list);
                editor.notice = Some(Notice::Reloaded);
                editor.touch(Instant::now());
            }
            None => editor.notice = Some(Notice::Failed(tr("This list was deleted elsewhere.").into())),
        }
    }

    /// The dialog in a window of `class` whose logical size is `window`: over the window (wide,
    /// medium; the question always), as a view of its own in narrow ones.
    pub fn view(&self, class: SizeClass, window: Size) -> Element<'_, Message> {
        match self {
            Dialog::Delete(delete) => delete.view(),
            Dialog::Edit(editor) if matches!(class, SizeClass::Wide | SizeClass::Medium) => editor.dialog(window),
            Dialog::Edit(editor) => editor.full(window.width),
        }
    }

    /// Whether the dialog covers the whole window (the editor in a narrow one).
    pub fn is_full(&self, class: SizeClass) -> bool {
        matches!(self, Dialog::Edit(_)) && !matches!(class, SizeClass::Wide | SizeClass::Medium)
    }
}

impl Editor {
    /// Takes name, colour, searches and revision of `list`.
    fn load(&mut self, list: &Collection) {
        let Some(source) = list.source.as_ref() else { return };
        if let Target::Edit { revision, .. } = &mut self.target {
            *revision = source.revision;
        }
        self.name = list.name.clone();
        self.color = list.color.clone();
        self.searches = source.queries.iter().map(|q| Search::new(q)).collect();
        if self.searches.is_empty() {
            self.searches.push(Search::new(""));
        }
        self.current = 0;
    }

    fn search(&self) -> &Search {
        &self.searches[self.current.min(self.searches.len() - 1)]
    }

    fn search_mut(&mut self) -> &mut Search {
        let at = self.current.min(self.searches.len() - 1);
        &mut self.searches[at]
    }

    /// The searches as they are sent: trimmed, empty ones left out.
    fn queries(&self) -> Vec<String> {
        self.searches.iter().map(|s| s.query.trim().to_string()).filter(|q| !q.is_empty()).collect()
    }

    /// The searches changed at `now`: the preview waits [`DEBOUNCE`] for the next change.
    fn touch(&mut self, now: Instant) {
        self.changed = Some(now);
    }

    /// The searches to preview at `now`, if the wait is over; the wait ends either way. `None`
    /// while it lasts, and when there is no search (the old preview goes).
    fn due(&mut self, now: Instant) -> Option<Vec<String>> {
        let changed = self.changed?;
        if now.saturating_duration_since(changed) < DEBOUNCE {
            return None;
        }
        self.changed = None;
        let queries = self.queries();
        if queries.is_empty() {
            self.preview = None;
            return None;
        }
        Some(queries)
    }

    /// Why the name or the searches cannot be saved, once Save was pressed.
    fn name_problem(&self) -> Option<&'static str> {
        (self.tried && self.name.trim().is_empty()).then(|| tr("Enter a name."))
    }

    fn query_problem(&self) -> Option<&'static str> {
        (self.tried && self.queries().is_empty()).then(|| tr("Enter a search or paste a github.com address."))
    }

    fn update(&mut self, message: Message, source: &Source) -> (Task<Message>, Outcome) {
        let stay = |task| (task, Outcome::Stay);
        match message {
            Message::Name(name) => self.name = name,
            Message::Color(color) => self.color = color,
            Message::Template(template) => {
                *self.search_mut() = Search::new(template.query());
                if self.name.trim().is_empty() {
                    self.name = template.label().to_string();
                }
                self.touch(Instant::now());
            }
            Message::Select(index) => self.current = index.min(self.searches.len() - 1),
            Message::AddSearch => {
                if self.searches.len() < MAX_SEARCHES {
                    self.searches.push(Search::new(""));
                    self.current = self.searches.len() - 1;
                }
            }
            Message::RemoveSearch(index) => {
                if self.searches.len() > 1 && index < self.searches.len() {
                    self.searches.remove(index);
                    self.current = self.current.min(self.searches.len() - 1);
                    self.touch(Instant::now());
                }
            }
            Message::Paste(text) => self.paste = text,
            Message::Pasted(text) => {
                let text = if text.trim().is_empty() { std::mem::take(&mut self.paste) } else { text };
                self.paste.clear();
                if !text.trim().is_empty() {
                    *self.search_mut() = Search::new(&text);
                    self.touch(Instant::now());
                }
            }
            Message::Kind(kind) => self.change(|query| kind.put(query)),
            Message::State(state) => self.change(|query| state.put(query)),
            Message::Sort(sort) => self.change(|query| sort.put(query)),
            Message::Text(field, text) => {
                let before = self.search().query.clone();
                self.search_mut().set_text(field, text);
                if self.search().query != before {
                    self.touch(Instant::now());
                }
            }
            Message::Query(query) => {
                *self.search_mut() = Search { query: query.clone(), ..Search::new(&query) };
                self.touch(Instant::now());
            }
            Message::Tick(now) => return stay(self.preview_task(now, source)),
            Message::Previewed(asked, result) => {
                // An answer to an older state of the searches is no use any more.
                if asked == self.asked {
                    self.loading = false;
                    self.preview = Some(result);
                }
            }
            Message::Save => return stay(self.save(source)),
            Message::Saved(result) => {
                self.saving = false;
                return match result {
                    Ok(id) => (Task::none(), Outcome::Saved(id)),
                    Err(ProviderError::Conflict { .. }) => {
                        self.notice = Some(Notice::Conflict);
                        (Task::none(), Outcome::Reload)
                    }
                    Err(err) => {
                        self.notice = Some(Notice::Failed(problem_text(&err)));
                        stay(Task::none())
                    }
                };
            }
            Message::Cancel => return (Task::none(), Outcome::Close),
            Message::Backdrop | Message::ConfirmDelete | Message::Deleted(_) => {}
        }
        stay(Task::none())
    }

    /// Rewrites the search being edited with `change` (a choice of the form) and re-reads the
    /// text fields; a preview follows when the search changed.
    fn change(&mut self, change: impl FnOnce(&str) -> String) {
        let query = change(&self.search().query);
        if query != self.search().query {
            *self.search_mut() = Search::new(&query);
            self.touch(Instant::now());
        }
    }

    /// Asks the core for the preview when the wait is over at `now`.
    fn preview_task(&mut self, now: Instant, source: &Source) -> Task<Message> {
        let Some(queries) = self.due(now) else { return Task::none() };
        self.asked += 1;
        self.loading = true;
        let (asked, source, account) = (self.asked, source.clone(), self.account.clone());
        Task::perform(
            async move { source.preview_filter(account, queries).await.map_err(|err| problem_text(&err)) },
            move |result| Message::Previewed(asked, result),
        )
    }

    /// Sends the list, or – when name or searches are missing – shows the errors and focuses the
    /// first field with one. Save stays pressable so it can point there.
    fn save(&mut self, source: &Source) -> Task<Message> {
        self.tried = true;
        if self.name_problem().is_some() {
            return operation::focus(NAME_INPUT);
        }
        if self.query_problem().is_some() {
            return operation::focus(QUERY_INPUT);
        }
        if self.saving {
            return Task::none();
        }
        self.saving = true;
        if self.notice != Some(Notice::Reloaded) {
            self.notice = None;
        }
        let definition =
            FilterDefinition { name: self.name.trim().to_string(), color: self.color.clone(), queries: self.queries() };
        let source = source.clone();
        match self.target.clone() {
            Target::New => {
                let account = self.account.clone();
                Task::perform(
                    async move { source.create_filter_list(account, definition).await.map(|list| list.id) },
                    Message::Saved,
                )
            }
            Target::Edit { collection, revision } => Task::perform(
                async move { source.update_filter_list(collection, revision, definition).await.map(|list| list.id) },
                Message::Saved,
            ),
        }
    }

    fn title(&self) -> &'static str {
        match self.target {
            Target::New => tr("New filter list"),
            Target::Edit { .. } => tr("Edit filter list"),
        }
    }

    // --- view ---

    /// Over the window: the card on the dialog surface, at most [`size::FORM_MAX`] wide and
    /// [`dialog_height`] high, its middle scrolling.
    fn dialog(&self, window: Size) -> Element<'_, Message> {
        let head = row![
            column![
                typo::text(typo::TITLE, self.title()),
                typo::text(typo::BODY, self.account_label.as_str()).style(theme::text_2),
            ]
            .spacing(space::XS)
            .width(Fill),
            widgets::icon_button(Icon::X, tr("Close"), Some(Message::Cancel)),
        ]
        .align_y(Alignment::Start)
        .padding(Padding { top: space::XL, right: space::L, bottom: space::L, left: space::XL });
        let card = column![
            head,
            rule::horizontal(LINE).style(style::rule),
            self.scrolled(space::XL, false, (window.width - 2.0 * space::L).min(size::FORM_MAX) - 2.0 * space::XL),
            rule::horizontal(LINE).style(style::rule),
            container(self.footer()).padding([space::L, space::XL]),
        ];
        let card = container(card).width(Fill).height(dialog_height(window.height)).style(style::container::popover);
        widgets::modal(Space::new().width(Fill).height(Fill), card, size::FORM_MAX, Message::Backdrop)
    }

    /// In a narrow window: a view of its own over the whole window, with a header bar ("✕ New
    /// filter list"), the form scrolling and the buttons at the bottom.
    fn full(&self, width: f32) -> Element<'_, Message> {
        let head = container(torro_gui::shell::header_bar(
            [widgets::icon_button(Icon::X, tr("Close"), Some(Message::Cancel))],
            torro_gui::shell::header_title(Some(Icon::ListFilter), self.title(), None),
            [],
        ))
        .center_y(size::HEADER)
        .padding([0.0, space::S]);
        let view = column![
            head,
            rule::horizontal(LINE).style(style::rule),
            self.scrolled(space::L, true, width - 2.0 * space::L),
            rule::horizontal(LINE).style(style::rule),
            container(self.footer()).padding([space::M, space::L]),
        ];
        container(view).width(Fill).height(Fill).style(style::container::panel_flush).into()
    }

    /// The body, scrolling, with `inset` around it.
    fn scrolled(&self, inset: f32, narrow: bool, width: f32) -> Element<'_, Message> {
        scrollable(container(self.body(narrow, width)).padding(inset).width(Fill))
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .height(Fill)
            .into()
    }

    /// Notice, name and colour, templates, the searches with the form, and the preview.
    fn body(&self, narrow: bool, width: f32) -> Element<'_, Message> {
        let mut parts: Vec<Element<'_, Message>> = Vec::new();
        if let Some(notice) = self.notice_view() {
            parts.push(notice);
        }
        if narrow {
            parts.push(column![self.name_field(), self.colors()].spacing(space::L).into());
        } else {
            parts.push(row![self.name_field(), self.colors()].spacing(space::L).align_y(Alignment::Start).into());
        }
        parts.push(self.templates());
        parts.push(self.searches_view(narrow));
        parts.push(self.preview_view(width));
        column(parts).spacing(space::XL).into()
    }

    fn notice_view(&self) -> Option<Element<'_, Message>> {
        let (which, tone, text): (Icon, Tone, String) = match self.notice.as_ref()? {
            Notice::Failed(reason) => (Icon::CircleAlert, Tone::Error, reason.clone()),
            Notice::Conflict => {
                (Icon::RefreshCw, Tone::Attention, tr("This list was changed elsewhere. Loading the current version…").into())
            }
            Notice::Reloaded => (
                Icon::CircleAlert,
                Tone::Attention,
                tr("This list was changed elsewhere. It shows the current version now; make your changes again and save.")
                    .into(),
            ),
        };
        let line = row![list::toned(which, icon::SIZE, tone), typo::text(typo::BODY, text).style(theme::text_2)]
            .spacing(space::S)
            .align_y(Alignment::Start);
        Some(container(line).padding(space::M).width(Fill).style(style::container::group).into())
    }

    fn name_field(&self) -> Element<'_, Message> {
        let input =
            field(tr("Name of the list"), &self.name).id(NAME_INPUT).on_input(Message::Name).on_submit(Message::Save);
        labelled(tr("Name"), input, self.name_problem()).width(Fill).into()
    }

    /// The default colour, the five list colours of the suite, and the list's own colour when it
    /// is none of them.
    fn colors(&self) -> Element<'_, Message> {
        let mut choices: Vec<Option<String>> = vec![None];
        choices.extend(list_colors().into_iter().map(Some));
        if let Some(own) = &self.color
            && !choices.iter().flatten().any(|c| c.eq_ignore_ascii_case(own))
        {
            choices.push(Some(own.clone()));
        }
        labelled(tr("Color"), swatches(choices, self.color.as_deref(), Message::Color), None).into()
    }

    fn templates(&self) -> Element<'_, Message> {
        let chips = Template::ALL
            .iter()
            .map(|&t| widgets::chip(Some(Mark::Icon(t.icon())), t.label(), Some(Message::Template(t))))
            .collect();
        labelled(tr("Start from"), widgets::chip_row(chips), None).into()
    }

    /// The searches of the list (as rows when there are several), the field to paste into, the
    /// form and the search it writes.
    fn searches_view(&self, narrow: bool) -> Element<'_, Message> {
        let several = self.searches.len() > 1;
        let add = (self.searches.len() < MAX_SEARCHES).then_some(Message::AddSearch);
        let title = if several { tr("Searches") } else { tr("Search") };
        let mut parts: Vec<Element<'_, Message>> = vec![
            row![
                typo::text(typo::CAPTION, title).style(theme::text_3).width(Fill),
                widgets::text_button(Some(Icon::Plus), tr("Add search"), style::button::ghost, add),
            ]
            .align_y(Alignment::Center)
            .into(),
        ];
        if several {
            parts.push(self.search_rows());
        }
        let paste = field(tr("Paste a search or a github.com address"), &self.paste)
            .on_input(Message::Paste)
            .on_paste(Message::Pasted)
            .on_submit(Message::Pasted(String::new()));
        parts.push(paste.into());
        let search = self.search();
        if is_address(&search.query) {
            parts.push(hint(
                Icon::Link,
                tr("This is an address on github.com. The list turns it into a search when you save."),
            ));
        } else {
            parts.push(self.form(search, narrow));
        }
        let query = field(tr("is:open is:issue repo:owner/name label:bug"), &search.query)
            .id(QUERY_INPUT)
            .on_input(Message::Query)
            .on_submit(Message::Save);
        parts.push(labelled(tr("GitHub search"), query, self.query_problem()).into());
        column(parts).spacing(space::M).into()
    }

    /// One row per search, the one being edited marked; "✕" removes one.
    fn search_rows(&self) -> Element<'_, Message> {
        let rows = self.searches.iter().enumerate().map(|(index, search)| {
            let label: Element<'_, Message> = if search.query.is_empty() {
                typo::text(typo::BODY, tr("Empty search")).style(theme::text_3).wrapping(Wrapping::None).into()
            } else {
                typo::text(typo::BODY, search.query.as_str()).wrapping(Wrapping::None).into()
            };
            let pick = button(container(label).center_y(Fill).clip(true))
                .width(Fill)
                .height(size::CONTROL)
                .padding(style::ITEM_PADDING)
                .style(style::button::list_row(index == self.current))
                .on_press(Message::Select(index));
            row![pick, widgets::icon_button(Icon::X, tr("Remove search"), Some(Message::RemoveSearch(index)))]
                .spacing(space::XS)
                .align_y(Alignment::Center)
                .into()
        });
        column![
            column(rows).spacing(space::XS),
            typo::text(typo::META, tr("The list shows what any of these searches finds.")).style(theme::text_2),
        ]
        .spacing(space::S)
        .into()
    }

    /// The form of `search`: two columns, one in narrow windows.
    fn form<'a>(&'a self, search: &'a Search, narrow: bool) -> Element<'a, Message> {
        let kind = Kind::of(&search.query);
        let kinds = [(tr("Issues"), Kind::Issues), (tr("Pull requests"), Kind::PullRequests), (tr("Both"), Kind::Both)];
        let kind_at = kinds.iter().position(|(_, k)| *k == kind).unwrap_or_default();
        let kind = widgets::segmented(kinds.map(|(label, k)| (label, Message::Kind(k))), kind_at);
        let state = State::of(&search.query);
        let states = [(tr("Open"), State::Open), (tr("Closed"), State::Closed), (tr("Any"), State::Any)];
        let state_at = states.iter().position(|(_, s)| *s == state).unwrap_or_default();
        let state = widgets::segmented(states.map(|(label, s)| (label, Message::State(s))), state_at);
        let sort = Sort::of(&search.query);
        let sort = pick_list(Sort::choices(&sort), Some(sort), Message::Sort)
            .width(Fill)
            .padding(style::PICK_LIST_PADDING)
            .text_size(typo::BODY.size)
            .text_line_height(typo::BODY.line_height)
            .font(typo::BODY.font)
            .style(style::pick_list)
            .menu_style(style::pick_list_menu);
        let text = |field: Field| {
            let input = self::field(field.placeholder(), search.text(field))
                .on_input(move |text| Message::Text(field, text))
                .on_submit(Message::Save);
            labelled(field.label(), input, None)
        };
        let pair = |a: Column<'a, Message>, b: Column<'a, Message>| -> Element<'a, Message> {
            if narrow {
                column![a, b].spacing(space::M).into()
            } else {
                row![a.width(Fill), b.width(Fill)].spacing(space::L).into()
            }
        };
        column![
            pair(labelled(tr("Type"), kind, None), labelled(tr("State"), state, None)),
            text(Field::Repos),
            pair(text(Field::Labels), text(Field::WithoutLabels)),
            pair(text(Field::Assignee), text(Field::Author)),
            pair(text(Field::Mentions), text(Field::ReviewRequested)),
            pair(text(Field::Milestone), labelled(tr("Sort"), sort, None)),
        ]
        .spacing(space::M)
        .into()
    }

    /// "Preview" and the first matches; what went wrong, if the core could not run the searches.
    fn preview_view(&self, width: f32) -> Element<'_, Message> {
        let title = typo::text(typo::CAPTION, tr("Preview")).style(theme::text_3);
        let body: Element<'_, Message> = match &self.preview {
            None if self.loading => quiet(tr("Checking…")),
            None => quiet(tr("The first matches show here once there is a search.")),
            Some(Err(reason)) => hint(Icon::CircleAlert, reason.clone()),
            Some(Ok(preview)) if preview.todos.is_empty() => quiet(tr("Nothing matches yet.")),
            Some(Ok(preview)) => column(preview.todos.iter().take(PREVIEW_ROWS).map(|todo| match_row(todo, width)))
                .spacing(space::XS)
                .into(),
        };
        column![title, body].spacing(space::S).into()
    }

    /// The count of the preview at the left, then "Cancel" and the one primary button.
    fn footer(&self) -> Element<'_, Message> {
        let status: Element<'_, Message> = match &self.preview {
            _ if self.loading && self.preview.is_none() => quiet(tr("Checking…")),
            Some(Ok(preview)) => {
                let mut count = matches_text(preview.matches);
                if preview.truncated {
                    count = trf("{count} · showing the first 1,000", &[("count", &count)]);
                }
                typo::text(typo::META.tabular(), count).style(theme::text_2).into()
            }
            Some(Err(_)) => row![
                list::toned(Icon::CircleAlert, icon::SIZE_SM, Tone::Attention),
                typo::text(typo::META, tr("No preview")).style(theme::text_2),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center)
            .into(),
            None => Space::new().into(),
        };
        let label = match self.target {
            Target::New => tr("Create list"),
            Target::Edit { .. } => tr("Save"),
        };
        let save = (!self.saving).then_some(Message::Save);
        row![
            container(status).width(Fill).clip(true),
            widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Cancel)),
            widgets::text_button(None, label, style::button::primary, save),
        ]
        .spacing(space::S)
        .align_y(Alignment::Center)
        .into()
    }
}

impl Delete {
    fn update(&mut self, message: Message, source: &Source) -> (Task<Message>, Outcome) {
        match message {
            Message::Cancel => (Task::none(), Outcome::Close),
            Message::ConfirmDelete if !self.busy => {
                self.busy = true;
                self.problem = None;
                let (source, collection) = (source.clone(), self.collection.clone());
                let task = Task::perform(
                    async move { source.delete_filter_list(collection).await.map_err(|err| problem_text(&err)) },
                    Message::Deleted,
                );
                (task, Outcome::Stay)
            }
            Message::Deleted(Ok(())) => (Task::none(), Outcome::Deleted(self.collection.clone())),
            Message::Deleted(Err(reason)) => {
                self.busy = false;
                self.problem = Some(reason);
                (Task::none(), Outcome::Stay)
            }
            _ => (Task::none(), Outcome::Stay),
        }
    }

    /// The question: title, what happens, "Cancel" and "Delete list" (danger). A click beside it
    /// cancels, as in the control centre.
    fn view(&self) -> Element<'_, Message> {
        let consequence =
            trf("“{name}” is removed from Torro Workplace. The issues stay on GitHub.", &[("name", &self.name)]);
        let mut content = column![
            typo::text(typo::TITLE, tr("Delete this filter list?")),
            typo::text(typo::BODY, consequence).style(theme::text_2),
        ]
        .spacing(space::L);
        if let Some(problem) = &self.problem {
            content = content.push(
                typo::text(typo::BODY, problem.as_str())
                    .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }),
            );
        }
        let confirm = (!self.busy).then_some(Message::ConfirmDelete);
        content = content.push(
            row![
                Space::new().width(Fill),
                widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Cancel)),
                widgets::text_button(None, tr("Delete list"), style::button::danger, confirm),
            ]
            .spacing(space::S)
            .align_y(Alignment::Center),
        );
        let card = container(content).padding(space::XL).width(Fill).style(style::container::popover);
        widgets::modal(Space::new().width(Fill).height(Fill), card, widgets::POPOVER_WIDTH, Message::Cancel)
    }
}

// ---------------------------------------------------------------------------------------------
// Small pieces
// ---------------------------------------------------------------------------------------------

/// A text field in the look of the suite: `BODY`, 32 high.
pub(crate) fn field<'a, M: Clone + 'a>(placeholder: &str, value: &str) -> text_input::TextInput<'a, M> {
    text_input(placeholder, value)
        .size(typo::BODY.size)
        .line_height(typo::BODY.line_height)
        .font(typo::BODY.font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
}

/// `label` (`META`, `text_2`) 4 above `control`, and the `problem` under it in the error colour.
pub(crate) fn labelled<'a, M: 'a>(
    label: &'a str,
    control: impl Into<Element<'a, M>>,
    problem: Option<&'a str>,
) -> Column<'a, M> {
    let mut column = column![typo::text(typo::META, label).style(theme::text_2), control.into()].spacing(space::XS);
    if let Some(problem) = problem {
        column = column.push(
            typo::text(typo::META, problem).style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }),
        );
    }
    column
}

/// The height of the dialog in a window `window_height` high: the window less the margin
/// `widgets::modal` keeps around it, at most [`MAX_HEIGHT`]. A fixed height, not `Fill` with a
/// maximum: `modal`'s own frame would take `Fill` from the card and draw its shadow down to the
/// window's bottom edge.
fn dialog_height(window_height: f32) -> f32 {
    (window_height - 2.0 * space::L).clamp(0.0, MAX_HEIGHT)
}

/// The five list colours of the suite as `#RRGGBB`, as the core stores them: a new list takes one
/// of them.
pub(crate) fn list_colors() -> Vec<String> {
    Palette::dark().calendar.iter().map(|&c| hex(c)).collect()
}

/// A row of colour swatches, one per `choices` (`None`: the default colour, drawn as a ring), each
/// 32 × 32 with a dot; the one equal to `chosen` (case does not matter) is marked by its surface,
/// not only by its colour. A click sends `pick` with the swatch's colour.
pub(crate) fn swatches<'a, M: Clone + 'a>(
    choices: Vec<Option<String>>,
    chosen: Option<&str>,
    pick: impl Fn(Option<String>) -> M,
) -> Element<'a, M> {
    let swatches = choices.into_iter().map(|choice| {
        let on = match (choice.as_deref(), chosen) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            (None, None) => true,
            _ => false,
        };
        // The default is a ring in the default colour: it is no colour of its own, and a dot
        // would look like a sixth blue.
        let dot = match &choice {
            Some(hex) => widgets::color_dot(collection_color(Some(hex)), icon::SIZE_SM),
            None => ring(collection_color(None)),
        };
        button(container(dot).center(Fill))
            .width(size::CONTROL)
            .height(size::CONTROL)
            .padding(0)
            .style(style::button::segment(on))
            .on_press(pick(choice))
            .into()
    });
    row(swatches).spacing(space::XS).into()
}

/// A ring of `colour` as large as a colour dot of the swatches, drawn like a list's ring.
fn ring<'a, M: 'a>(colour: Color) -> Element<'a, M> {
    container(Space::new().width(icon::SIZE_SM).height(icon::SIZE_SM))
        .style(move |_: &Theme| container::Style {
            border: iced::border::rounded(icon::SIZE_SM / 2.0).color(colour).width(style::CHECK_BORDER),
            ..container::Style::default()
        })
        .into()
}

/// A quiet line in `text_3`.
fn quiet<'a>(line: &'a str) -> Element<'a, Message> {
    typo::text(typo::BODY, line).style(theme::text_3).into()
}

/// An explaining line with an icon in `text_3` before it.
fn hint<'a>(which: Icon, line: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    row![list::toned(which, icon::SIZE, Tone::Text3), typo::text(typo::BODY, line).style(theme::text_2)]
        .spacing(space::S)
        .align_y(Alignment::Start)
        .into()
}

/// A match in the preview: its type symbol, title and `repo#123`, on one line in a body `width`
/// wide; a title that does not fit ends in "…".
fn match_row(todo: &Todo, width: f32) -> Element<'_, Message> {
    let mark: Element<'_, Message> = match list::github_of(todo) {
        Some(details) => {
            let (which, tone) = list::github_mark(details);
            list::toned(which, icon::SIZE, tone)
        }
        None => list::toned(Icon::CircleDot, icon::SIZE, Tone::Text3),
    };
    let reference = todo
        .link
        .as_ref()
        .and_then(|link| link.github_ref())
        .map(|(repo, number)| format!("{}#{number}", repo.rsplit('/').next().unwrap_or(repo)))
        .unwrap_or_default();
    let room = width - icon::SIZE - 2.0 * space::S - typo::text_width(&reference, typo::META);
    let title = typo::ellipsize(&todo.summary, typo::BODY, room.max(0.0));
    row![
        mark,
        container(typo::text(typo::BODY, title).wrapping(Wrapping::None)).width(Fill).clip(true),
        typo::text(typo::META.tabular(), reference).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center)
    .height(size::CONTROL_SM)
    .into()
}

/// "1 match", "12 matches".
pub fn matches_text(n: u64) -> String {
    if n == 1 { tr("1 match").to_string() } else { trf("{n} matches", &[("n", &n)]) }
}

/// What the dialog says when the core refused: GitHub's sign-in, the connection, else the core's
/// own message.
fn problem_text(err: &ProviderError) -> String {
    match err {
        ProviderError::Auth => tr("GitHub refused the sign-in. Check the GitHub account in Settings.").into(),
        ProviderError::Offline => tr("GitHub can't be reached. Check the connection and try again.").into(),
        ProviderError::Conflict { .. } => tr("This list was changed elsewhere.").into(),
        ProviderError::Other(message) => message.clone(),
    }
}

/// `color` as `#RRGGBB`, as the core stores colours.
pub(crate) fn hex(color: Color) -> String {
    let [r, g, b, _] = color.into_rgba8();
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::NaiveDate;
    use torro_pim::demo::{DemoSource, GITHUB_ACCOUNT, TORRO_BUGS};
    use torro_pim::model::GitHubFilter;

    use super::*;

    fn demo() -> Source {
        Source::Demo(Arc::new(DemoSource::new(NaiveDate::from_ymd_opt(2026, 10, 11).expect("valid date"))))
    }

    fn list(revision: u64, queries: &[&str]) -> Collection {
        Collection {
            id: TORRO_BUGS.into(),
            name: "Torro bugs".into(),
            kind: torro_pim::CollectionKind::Todos,
            color: Some("#E8A33D".into()),
            writable: false,
            source: Some(GitHubFilter {
                queries: queries.iter().map(|q| q.to_string()).collect(),
                pinned: Vec::new(),
                revision,
                truncated: false,
                problem: None,
            }),
        }
    }

    fn editor(dialog: &Dialog) -> &Editor {
        match dialog {
            Dialog::Edit(editor) => editor,
            Dialog::Delete(_) => panic!("not the editor"),
        }
    }

    fn editor_mut(dialog: &mut Dialog) -> &mut Editor {
        match dialog {
            Dialog::Edit(editor) => editor,
            Dialog::Delete(_) => panic!("not the editor"),
        }
    }

    fn new_dialog() -> Dialog {
        Dialog::new_list(GITHUB_ACCOUNT.into(), "GitHub · mahype".into())
    }

    /// Writes back what every part of the form reads: the search must not change at all.
    fn written_back(query: &str) -> String {
        let mut out = query.to_string();
        for field in Field::ALL {
            out = set_field(&out, field, &field_text(&out, field));
        }
        out = Kind::of(&out).put(&out);
        out = State::of(&out).put(&out);
        Sort::of(&out).put(&out)
    }

    #[test]
    fn words_keep_quoted_values_together() {
        assert_eq!(
            words("is:open  label:\"good first issue\" -label:wontfix  word"),
            ["is:open", "label:\"good first issue\"", "-label:wontfix", "word"]
        );
        assert_eq!(
            read("-label:\"needs info\""),
            Word { negated: true, qualifier: Some("label".into()), value: "needs info".into() }
        );
        assert_eq!(read("hello").qualifier, None);
    }

    #[test]
    fn the_form_reads_every_field() {
        let query = "is:open is:pr repo:mahype/a repo:mahype/b label:bug label:\"good first issue\" -label:wontfix \
                     assignee:@me author:anna mentions:bob review-requested:@me milestone:\"v1 beta\" sort:updated-desc";
        assert_eq!(Kind::of(query), Kind::PullRequests);
        assert_eq!(State::of(query), State::Open);
        assert_eq!(Sort::of(query), Sort::Updated);
        let read = |field| field_text(query, field);
        assert_eq!(read(Field::Repos), "mahype/a, mahype/b");
        assert_eq!(read(Field::Labels), "bug, good first issue");
        assert_eq!(read(Field::WithoutLabels), "wontfix");
        assert_eq!(read(Field::Assignee), "@me");
        assert_eq!(read(Field::Author), "anna");
        assert_eq!(read(Field::Mentions), "bob");
        assert_eq!(read(Field::ReviewRequested), "@me");
        assert_eq!(read(Field::Milestone), "v1 beta");
    }

    #[test]
    fn form_and_search_go_round_without_change() {
        for query in [
            "",
            "is:open is:issue repo:mahype/TorroWorkplace label:bug",
            "type:pr state:closed label:\"good first issue\" sort:created-asc",
            "is:open involves:@me",
            "label:bug,ui no:assignee \"free text\" org:mahype -repo:mahype/old OR (is:pr)",
            "is:open is:closed assignee:a assignee:b sort:reactions-+1-desc",
            "Repo:Mahype/A LABEL:Bug",
            "milestone:\"v1 beta\" label:\"bug\"",
        ] {
            assert_eq!(written_back(query), query, "{query}");
        }
    }

    #[test]
    fn unknown_parts_stay_where_they_are() {
        let query = "involves:@me label:bug,ui no:assignee \"free text\" label:bug org:mahype";
        let changed = set_field(query, Field::Labels, "bug, needs info");
        assert_eq!(
            changed,
            "involves:@me label:bug,ui no:assignee \"free text\" label:bug label:\"needs info\" org:mahype"
        );
        let issues = Kind::Issues.put(&changed);
        assert!(issues.ends_with("org:mahype is:issue") && issues.starts_with("involves:@me label:bug,ui"), "{issues}");
        // A second assignee is not the field's: it stays when the field changes.
        assert_eq!(set_field("assignee:a assignee:b", Field::Assignee, "@me"), "assignee:@me assignee:b");
    }

    #[test]
    fn fields_and_choices_rewrite_in_place() {
        let query = "is:open repo:mahype/a label:bug sort:updated-desc";
        assert_eq!(
            set_field(query, Field::Repos, "mahype/a mahype/b,"),
            "is:open repo:mahype/a repo:mahype/b label:bug sort:updated-desc"
        );
        assert_eq!(set_field(query, Field::Labels, ""), "is:open repo:mahype/a sort:updated-desc");
        assert_eq!(set_field(query, Field::Milestone, "v1 beta"), format!("{query} milestone:\"v1 beta\""));
        assert_eq!(State::Closed.put(query), "is:closed repo:mahype/a label:bug sort:updated-desc");
        assert_eq!(State::Any.put(query), "repo:mahype/a label:bug sort:updated-desc");
        assert_eq!(Sort::BestMatch.put(query), "is:open repo:mahype/a label:bug");
        assert_eq!(Sort::Comments.put(query), "is:open repo:mahype/a label:bug sort:comments-desc");
        assert_eq!(Kind::Both.put("is:pr is:open"), "is:open");
    }

    #[test]
    fn typing_keeps_the_text_and_writes_the_search() {
        let mut search = Search::new("is:open");
        search.set_text(Field::Labels, "bug, ".into());
        assert_eq!((search.query.as_str(), search.text(Field::Labels)), ("is:open label:bug", "bug, "));
        search.set_text(Field::Labels, "bug, ui".into());
        assert_eq!(search.query, "is:open label:bug label:ui");
    }

    #[test]
    fn templates_write_their_search_and_name_an_unnamed_list() {
        let queries: Vec<&str> = Template::ALL.iter().map(|t| t.query()).collect();
        assert_eq!(
            queries,
            [
                "is:open assignee:@me",
                "is:open is:pr review-requested:@me",
                "is:open is:pr author:@me",
                "is:open mentions:@me",
            ]
        );
        let mut dialog = new_dialog();
        let _ = dialog.update(Message::Template(Template::ReviewRequested), &demo());
        let e = editor(&dialog);
        assert_eq!(
            (e.search().query.as_str(), e.name.as_str()),
            (Template::ReviewRequested.query(), tr("Review requested"))
        );
        assert_eq!(e.search().text(Field::ReviewRequested), "@me");
        // A name of one's own stays.
        let mut dialog = new_dialog();
        let _ = dialog.update(Message::Name("Mine".into()), &demo());
        let _ = dialog.update(Message::Template(Template::Mentioned), &demo());
        assert_eq!(editor(&dialog).name, "Mine");
    }

    #[test]
    fn an_address_becomes_the_search_as_it_is() {
        let address = "https://github.com/mahype/TorroWorkplace/issues?q=is%3Aopen+label%3Abug";
        assert!(is_address(address) && is_address("github.com/issues") && !is_address("is:open repo:a/b"));
        let mut dialog = new_dialog();
        let _ = dialog.update(Message::Pasted(address.into()), &demo());
        let e = editor(&dialog);
        assert_eq!(e.search().query, address, "the core turns it into a search");
        assert!(e.changed.is_some(), "a preview follows");
        assert!(e.paste.is_empty());
        // Enter in the field takes what was typed.
        let mut dialog = new_dialog();
        let _ = dialog.update(Message::Paste("is:open label:bug".into()), &demo());
        let _ = dialog.update(Message::Pasted(String::new()), &demo());
        assert_eq!(editor(&dialog).search().text(Field::Labels), "bug");
    }

    #[test]
    fn the_preview_waits_for_the_typing_to_stop() {
        let mut dialog = new_dialog();
        let editor = editor_mut(&mut dialog);
        let start = Instant::now();
        assert_eq!(editor.due(start + DEBOUNCE), None, "nothing changed");
        editor.searches[0] = Search::new("is:open");
        editor.touch(start);
        assert_eq!(editor.due(start + DEBOUNCE / 2), None, "still typing");
        editor.touch(start + DEBOUNCE / 2);
        assert_eq!(editor.due(start + DEBOUNCE), None, "the wait starts again with every change");
        assert_eq!(editor.due(start + DEBOUNCE / 2 + DEBOUNCE), Some(vec!["is:open".to_string()]));
        assert_eq!(editor.due(start + 3 * DEBOUNCE), None, "asked once");
        // Without a search the old preview goes and nothing is asked.
        editor.preview = Some(Ok(FilterPreview::default()));
        editor.searches[0] = Search::new(" ");
        editor.touch(start);
        assert_eq!(editor.due(start + DEBOUNCE), None);
        assert!(editor.preview.is_none());
    }

    #[test]
    fn only_the_newest_preview_counts() {
        let mut dialog = new_dialog();
        let source = demo();
        let start = Instant::now();
        let _ = dialog.update(Message::Query("is:open".into()), &source);
        editor_mut(&mut dialog).changed = Some(start);
        let _ = dialog.update(Message::Tick(start + DEBOUNCE), &source);
        let _ = dialog.update(Message::Query("is:closed".into()), &source);
        editor_mut(&mut dialog).changed = Some(start);
        let _ = dialog.update(Message::Tick(start + DEBOUNCE), &source);
        assert_eq!(editor(&dialog).asked, 2);
        let old = FilterPreview { matches: 7, ..FilterPreview::default() };
        let _ = dialog.update(Message::Previewed(1, Ok(old)), &source);
        assert!(editor(&dialog).preview.is_none() && editor(&dialog).loading, "the old answer is dropped");
        let new = FilterPreview { matches: 3, ..FilterPreview::default() };
        let _ = dialog.update(Message::Previewed(2, Ok(new.clone())), &source);
        assert_eq!(editor(&dialog).preview, Some(Ok(new)));
        assert_eq!(matches_text(3), trf("{n} matches", &[("n", &3)]));
        assert_eq!(matches_text(1), tr("1 match"));
    }

    #[test]
    fn searches_can_be_added_up_to_ten_and_removed_down_to_one() {
        let mut dialog = new_dialog();
        for _ in 0..12 {
            let _ = dialog.update(Message::AddSearch, &demo());
        }
        assert_eq!(editor(&dialog).searches.len(), MAX_SEARCHES);
        assert_eq!(editor(&dialog).current, MAX_SEARCHES - 1, "the new search is edited");
        for _ in 0..12 {
            let _ = dialog.update(Message::RemoveSearch(0), &demo());
        }
        assert_eq!(editor(&dialog).searches.len(), 1);
    }

    #[test]
    fn saving_needs_a_name_and_a_search() {
        let mut dialog = new_dialog();
        assert_eq!(editor(&dialog).name_problem(), None, "no error before the first try");
        let (_, outcome) = dialog.update(Message::Save, &demo());
        assert_eq!(outcome, Outcome::Stay);
        let editor_now = editor(&dialog);
        assert!(!editor_now.saving);
        assert_eq!(editor_now.name_problem(), Some(tr("Enter a name.")));
        assert!(editor_now.query_problem().is_some());
        let _ = dialog.update(Message::Name("Bugs".into()), &demo());
        let _ = dialog.update(Message::Query("label:bug".into()), &demo());
        let _ = dialog.update(Message::Save, &demo());
        assert!(editor(&dialog).saving);
        let (_, outcome) = dialog.update(Message::Saved(Ok(TORRO_BUGS.into())), &demo());
        assert_eq!(outcome, Outcome::Saved(TORRO_BUGS.into()));
    }

    #[test]
    fn edit_reads_the_list_and_sends_its_revision() {
        let dialog = Dialog::edit(&list(3, &["is:issue label:bug", "is:pr"]), GITHUB_ACCOUNT.into(), String::new());
        let dialog = dialog.expect("a filter list");
        let editor = editor(&dialog);
        assert_eq!(editor.target, Target::Edit { collection: TORRO_BUGS.into(), revision: 3 });
        assert_eq!((editor.name.as_str(), editor.color.as_deref()), ("Torro bugs", Some("#E8A33D")));
        assert_eq!(editor.queries(), ["is:issue label:bug", "is:pr"]);
        assert!(editor.changed.is_some(), "its preview follows");
        let plain = Collection { source: None, ..list(1, &[]) };
        assert!(Dialog::edit(&plain, GITHUB_ACCOUNT.into(), String::new()).is_none());
    }

    #[test]
    fn a_conflict_is_explained_and_the_current_version_loaded() {
        let mut dialog = Dialog::edit(&list(3, &["label:bug"]), GITHUB_ACCOUNT.into(), String::new()).expect("dialog");
        let _ = dialog.update(Message::Name("Mine".into()), &demo());
        let _ = dialog.update(Message::Save, &demo());
        // A reload before the conflict changes nothing.
        dialog.reloaded(&[list(4, &["label:other"])]);
        assert_eq!(editor(&dialog).name, "Mine");
        let conflict = ProviderError::Conflict { uid: TORRO_BUGS.into() };
        let (_, outcome) = dialog.update(Message::Saved(Err(conflict)), &demo());
        assert_eq!(outcome, Outcome::Reload);
        assert_eq!(editor(&dialog).notice, Some(Notice::Conflict));
        assert!(!editor(&dialog).saving);
        dialog.reloaded(&[list(4, &["label:other"])]);
        let editor = editor(&dialog);
        assert_eq!(editor.notice, Some(Notice::Reloaded));
        assert_eq!(editor.target, Target::Edit { collection: TORRO_BUGS.into(), revision: 4 });
        assert_eq!((editor.name.as_str(), editor.queries()), ("Torro bugs", vec!["label:other".to_string()]));
    }

    #[test]
    fn other_save_errors_are_shown_and_keep_the_dialog() {
        let mut dialog = new_dialog();
        let (_, outcome) = dialog.update(Message::Saved(Err(ProviderError::Offline)), &demo());
        assert_eq!(outcome, Outcome::Stay);
        assert_eq!(editor(&dialog).notice, Some(Notice::Failed(problem_text(&ProviderError::Offline))));
        assert_eq!(dialog.update(Message::Backdrop, &demo()).1, Outcome::Stay, "a stray click keeps the form");
        assert_eq!(dialog.update(Message::Cancel, &demo()).1, Outcome::Close);
    }

    #[test]
    fn deleting_asks_first() {
        let mut dialog = Dialog::delete(&list(1, &["label:bug"])).expect("a filter list");
        let Dialog::Delete(question) = &dialog else { panic!("the question") };
        assert!(!question.busy, "nothing is deleted before the answer");
        assert_eq!(dialog.clone().update(Message::Cancel, &demo()).1, Outcome::Close);
        let (_, outcome) = dialog.update(Message::ConfirmDelete, &demo());
        assert_eq!(outcome, Outcome::Stay);
        let Dialog::Delete(question) = &dialog else { panic!("the question") };
        assert!(question.busy);
        let (_, outcome) = dialog.update(Message::Deleted(Err("gone".into())), &demo());
        assert_eq!(outcome, Outcome::Stay);
        let (_, outcome) = dialog.update(Message::Deleted(Ok(())), &demo());
        assert_eq!(outcome, Outcome::Deleted(TORRO_BUGS.into()));
        assert!(Dialog::delete(&Collection { source: None, ..list(1, &[]) }).is_none());
    }

    #[test]
    fn colours_are_written_as_hex() {
        assert_eq!(hex(Palette::dark().calendar[0]), "#5B8CFF");
    }

    #[test]
    fn the_dialog_builds_in_every_layout() {
        let mut dialog = Dialog::edit(
            &list(1, &["is:open label:bug", "https://github.com/a/b/issues"]),
            GITHUB_ACCOUNT.into(),
            "GitHub".into(),
        )
        .expect("dialog");
        editor_mut(&mut dialog).preview = Some(Ok(FilterPreview { matches: 1, truncated: true, todos: Vec::new() }));
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            let _ = dialog.view(class, Size::new(1600.0, 840.0));
        }
        assert!(dialog.is_full(SizeClass::Narrow) && !dialog.is_full(SizeClass::Wide));
        let question = Dialog::delete(&list(1, &[])).expect("dialog");
        let _ = question.view(SizeClass::Narrow, Size::new(480.0, 840.0));
        assert!(!question.is_full(SizeClass::Narrow));
    }
}
