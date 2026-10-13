//! The dialog of a GitHub filter list (tasks G.4 and G.11 in `docs/tasks/github.md`, concept
//! "Filterliste" in `docs/github.md`): a new filter list ("New list…" › "GitHub filter list",
//! `new_list.rs`), "Edit filter…" and the question before "Delete list…".
//!
//! **The search is the truth.** A filter list is saved GitHub searches; everything in the dialog
//! only helps to write them. The form reads and rewrites only the words it knows ([`Field`],
//! [`Kind`], [`State`], [`Sort`], [`Pick`]), in place; every other word of the search stays exactly
//! where it was. Under "GitHub search" the search itself can be changed by hand, a search or the
//! address of an issue list on github.com pasted (the core turns addresses into searches when it
//! previews or saves), and further searches added.
//!
//! **Repository first.** The form starts with the repository, picked from those the account
//! reaches (`tasks.list_github_repos`) in a list with a field to find one. A new list preselects
//! the repository chosen last – for all accounts alike, kept by the app in `ui.json` – or, where
//! the account does not reach it, the account's first. With several GitHub accounts a new list
//! shows them as chips over the repository. Labels, milestones and people come from the repository
//! (`tasks.describe_github_repo`) and are clicked, not typed: labels as chips with their colour
//! (an item must have all chosen ones), assignee and milestone as chips, type and state as
//! segments; author, review requested, mentioned, sort and the labels an item must not have under
//! "More filters" (open when something of it is set). Changing the repository takes out the
//! labels and the milestone it does not have. While the values load, quiet placeholders show;
//! when they cannot be read, why, with "Retry" – the search under "GitHub search" stays usable.
//!
//! **Name:** suggested from the filters ("Review requested · TorroWorkplace") until one is typed.
//! A list may run up to [`MAX_SEARCHES`] searches; it holds what any of them finds. With several
//! searches, or several repositories in one, the repository field says "Several repositories" and
//! "GitHub search" is open. Templates (Assigned to me, Review requested, My open pull requests,
//! Mentioned) replace the search being edited, keep its repository and name an unnamed list.
//!
//! **Preview:** "{n} matches" and the first matches come from the core (`tasks.preview_filter`),
//! which asks GitHub once per search – so not on every click: a change waits [`DEBOUNCE`] for the
//! next one, and only the newest answer counts.
//!
//! **Saving** sends `tasks.create_filter_list` or `tasks.update_filter_list` with the revision the
//! dialog was opened with. When someone changed the list meanwhile the core answers `conflict`:
//! the dialog says so, the app reloads, and the dialog shows the current version
//! ([`Dialog::reloaded`]). Errors of the form show after the first try to save. After a save the
//! app remembers the repository ([`Dialog::repo_to_remember`]).
//!
//! Wide windows show the dialog over the window with the preview as a column beside the form,
//! medium ones with the preview under it, narrow windows as a view of its own over the whole
//! window. "Delete list…" asks first ("The issues stay on GitHub.").
//!
//! The app holds an `Option<Dialog>`, runs [`Dialog::load`] when it opens it, sends its
//! [`Message`]s to [`Dialog::update`] and acts on the [`Outcome`]; requests go out through the
//! app's [`Source`].

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use iced::time::Instant;
use iced::widget::text::Wrapping;
use iced::widget::{Column, Space, button, column, container, operation, row, rule, scrollable, text, text_input};
use iced::{Alignment, Background, Color, Element, Fill, Padding, Size, Subscription, Task, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::source::Source;
use torro_gui::theme::{self, Palette, palette, size, space};
use torro_gui::widgets::{self, Mark, collection_color};
use torro_gui::{style, typo};
use torro_pim::model::GitHubLabel;
use torro_pim::source::{FilterDefinition, FilterPreview, GitHubRepo, GitHubRepoDetails};
use torro_pim::{Collection, PimSource, ProviderError, Todo};

use crate::list::{self, Tone};
use crate::new_list::AccountChoice;

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
/// Width of the lines between head, form, preview and foot.
const LINE: f32 = size::HAIRLINE;
/// The dialog with the preview beside the form: the form at its reading width, the preview column.
const WIDTH: f32 = size::FORM_MAX + size::PROPS;
/// The search of a new list until a template or the form changes it: open issues.
const NEW_SEARCH: &str = "is:open is:issue";
/// How many repositories the open list of the repository field shows; typing finds the others.
const REPO_ROWS: usize = 8;
/// Widths of the quiet chip-shaped placeholders while the labels of a repository load.
const PLACEHOLDERS: [f32; 4] =
    [size::CONTROL * 2.0, size::CONTROL_LG * 2.0, size::CONTROL * 3.0, size::CONTROL_LG * 2.5];

/// Id of the name field: focused when a new list opens and when the name is missing on save.
pub const NAME_INPUT: &str = "filter-name";
/// Id of the search field: focused when no search is there on save.
const QUERY_INPUT: &str = "filter-query";
/// Id of the field that finds a repository: focused when the list of repositories opens.
const REPO_INPUT: &str = "filter-repo";
/// Id of the scrolling form, for `--open filter-search`, which shows its end.
pub const FORM_SCROLL: &str = "filter-form";

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

/// A filter of the form – chips, segments, the repository field – and the qualifier it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    /// `repo:owner/name`, several (any of them).
    Repos,
    /// `label:name`, several (all of them).
    Labels,
    /// `-label:name`, several.
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
    /// Every field, for the round-trip tests (the text fields of G.4 that wrote them are gone).
    #[cfg(test)]
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

    /// The values typed into a field: split at commas (repositories also at spaces), trimmed.
    #[cfg(test)]
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
}

/// The values of the words of `query` that `field` reads.
fn field_values(query: &str, field: Field) -> Vec<String> {
    let all = words(query);
    field.owned(query).iter().map(|&i| read(&all[i]).value).collect()
}

/// What `field` reads from `query`, as a text field showed it: values joined by ", ".
#[cfg(test)]
pub fn field_text(query: &str, field: Field) -> String {
    field_values(query, field).join(", ")
}

/// `query` with the words of `field` set from `text` (as typed into a text field), in place of the
/// old ones; unchanged when the values are the same, so writing back what was read never changes
/// the search. An empty `text` removes the field's words.
#[cfg(test)]
pub fn set_field(query: &str, field: Field, text: &str) -> String {
    set_values(query, field, &field.values(text))
}

/// `query` with the words of `field` set to `values` (as they are, no splitting), in place of the
/// old ones; unchanged when the values are the same. No values remove the field's words.
pub fn set_values(query: &str, field: Field, values: &[String]) -> String {
    if values == field_values(query, field).as_slice() {
        return query.to_string();
    }
    let new = values.iter().map(|value| write(field.qualifier(), field.negated(), value)).collect();
    replace(query, &field.owned(query), new)
}

/// The repositories `query` searches in (its `repo:` words), as written.
pub fn repos_of(query: &str) -> Vec<String> {
    field_values(query, Field::Repos)
}

/// `query` with the label `name` chosen in `side` ([`Field::Labels`]: must have it,
/// [`Field::WithoutLabels`]: must not) or, when it is chosen there already, no longer chosen. A
/// label chosen on one side leaves the other. Case does not matter, as on GitHub.
pub fn toggle_label(query: &str, side: Field, name: &str) -> String {
    let other = if side == Field::Labels { Field::WithoutLabels } else { Field::Labels };
    let mut values = field_values(query, side);
    if values.iter().any(|value| value.eq_ignore_ascii_case(name)) {
        values.retain(|value| !value.eq_ignore_ascii_case(name));
        return set_values(query, side, &values);
    }
    values.push(name.to_string());
    let query = set_values(query, side, &values);
    let rest: Vec<String> = field_values(&query, other).into_iter().filter(|v| !v.eq_ignore_ascii_case(name)).collect();
    set_values(&query, other, &rest)
}

/// What a filter of one value – [`Field::Assignee`], [`Field::Author`], [`Field::ReviewRequested`],
/// [`Field::Milestone`] – is set to in a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// No word: anyone, any milestone.
    Any,
    /// `@me` (people only): the signed-in account.
    Me,
    /// `no:assignee`, `no:milestone`: not set at all ("Unassigned", "No milestone").
    Nothing,
    /// A login or the title of a milestone.
    Value(String),
}

impl Field {
    /// The value of the `no:` word that means "not set" for this field; `None` where GitHub has none.
    fn nothing(self) -> Option<&'static str> {
        match self {
            Field::Assignee => Some("assignee"),
            Field::Milestone => Some("milestone"),
            _ => None,
        }
    }
}

/// The pick of `field` in `query` and where its word is: the first word of the field
/// ([`Field::owned`]) or its `no:` word, whichever comes first.
fn find_pick(query: &str, field: Field) -> (Pick, Option<usize>) {
    for (index, word) in words(query).iter().enumerate() {
        let word = read(word);
        if field.takes(&word) {
            let me = field != Field::Milestone && word.value.eq_ignore_ascii_case("@me");
            return (if me { Pick::Me } else { Pick::Value(word.value) }, Some(index));
        }
        let none = !word.negated && word.qualifier.as_deref() == Some("no");
        if none && field.nothing().is_some_and(|what| word.value.eq_ignore_ascii_case(what)) {
            return (Pick::Nothing, Some(index));
        }
    }
    (Pick::Any, None)
}

/// What `field` (a filter of one value) is set to in `query`.
pub fn pick_of(query: &str, field: Field) -> Pick {
    find_pick(query, field).0
}

/// `query` with `field` set to `pick`, in place of the old word; unchanged when it is the same.
/// [`Pick::Nothing`] for a field without a `no:` word counts as [`Pick::Any`].
pub fn set_pick(query: &str, field: Field, pick: &Pick) -> String {
    let (old, at) = find_pick(query, field);
    if old == *pick {
        return query.to_string();
    }
    let word = match pick {
        Pick::Any => None,
        Pick::Me => Some(write(field.qualifier(), false, "@me")),
        Pick::Value(value) => Some(write(field.qualifier(), false, value)),
        Pick::Nothing => field.nothing().map(|what| format!("no:{what}")),
    };
    replace(query, &at.into_iter().collect::<Vec<_>>(), word.into_iter().collect())
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
    /// The segments of "Sort": best match, newest, recently updated – and `current` when it is
    /// another order (oldest, most commented, one typed by hand), so it shows as chosen.
    fn choices(current: &Sort) -> Vec<Sort> {
        let mut all = vec![Sort::BestMatch, Sort::Newest, Sort::Updated];
        if !all.contains(current) {
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

// ---------------------------------------------------------------------------------------------
// The dialog
// ---------------------------------------------------------------------------------------------

/// How the app opens the dialog (sidebar, the list's "…" menu, `--open`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Open {
    /// A new filter list, first for the GitHub account with this id ("New list…" › "GitHub filter
    /// list").
    New(String),
    /// "Edit filter…" of the filter list with this collection id.
    Edit(String),
    /// "Delete list…" of the filter list with this collection id.
    Delete(String),
}

/// What a new filter list starts from ([`Dialog::new_list`]); the app builds it from its accounts
/// and `ui.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    /// The connected GitHub accounts, as the sidebar names them. With more than one the dialog
    /// shows them as chips over the repository; with none there is no dialog.
    pub accounts: Vec<AccountChoice>,
    /// The id of the account chosen at first; the first of `accounts` when it is none of them.
    pub account: String,
    /// The repository chosen last, for any account (`ui_state::last_repo_from`): preselected when
    /// the chosen account reaches it, else the account's first repository is.
    pub last_repo: Option<String>,
    /// Opened from the choice of "New list…": the head has a back arrow to it ([`Outcome::Back`]).
    pub back: bool,
}

/// What [`Dialog::update`] asks the app to do besides running the returned task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Keep the dialog open.
    Stay,
    /// Close the dialog; nothing changed.
    Close,
    /// The back arrow: close the dialog and show the choice of "New list…" again.
    Back,
    /// The list with this id was created or changed: close, show it, reload, and remember
    /// [`Dialog::repo_to_remember`].
    Saved(String),
    /// The list with this id was deleted: close, leave it if it is shown, reload.
    Deleted(String),
    /// Reload the collections and hand them to [`Dialog::reloaded`] (after a conflict).
    Reload,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A chip of the account row (a new list with several GitHub accounts).
    Account(String),
    /// The core's answer to `tasks.list_github_repos` for the account with this id.
    Repos(String, Result<Vec<GitHubRepo>, String>),
    /// The core's answer to `tasks.describe_github_repo`: account id, the repository as asked (in
    /// lower case), and labels, milestones and people – or why not.
    Described {
        account: String,
        repo: String,
        result: Result<GitHubRepoDetails, String>,
    },
    /// "Retry" under the repository field.
    RetryRepos,
    /// "Retry" where the labels of the repository failed to load.
    RetryDetails,
    /// The repository field: open or close the list of repositories.
    TogglePicker,
    /// Typed into the field that finds a repository.
    FindRepo(String),
    /// A repository of the list (`owner/name`).
    PickRepo(String),
    /// Enter in the field that finds a repository: the first one found.
    PickFound,
    Template(Template),
    Kind(Kind),
    State(State),
    Sort(Sort),
    /// A label chip: must have the label, or no longer.
    Label(String),
    /// A label chip under "Without labels": must not have the label, or no longer.
    Without(String),
    /// A chip or segment of a filter of one value (assignee, milestone, author, review requested).
    Pick(Field, Pick),
    /// "More filters": open or close.
    ToggleMore,
    /// "GitHub search": open or close.
    ToggleSearch,
    Name(String),
    Color(Option<String>),
    /// Edit the search at this index.
    Select(usize),
    AddSearch,
    RemoveSearch(usize),
    /// The search typed by hand (or a github.com address pasted).
    Query(String),
    /// While a preview waits: is the wait over?
    Tick(Instant),
    /// The core's answer to the preview asked for as the given one.
    Previewed(u64, Result<FilterPreview, String>),
    Save,
    /// The core's answer to Save: the id of the list as stored.
    Saved(Result<String, ProviderError>),
    /// The back arrow in the head (only when opened from "New list…").
    Back,
    /// Escape: closes the list of repositories when it is open, else the dialog.
    Escape,
    /// Cancel and the close button.
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

/// Something the dialog asked the core for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Load<T> {
    Loading,
    Ready(T),
    /// What went wrong, for a hint with "Retry".
    Failed(String),
}

impl<T> Load<T> {
    fn of(result: Result<T, String>) -> Load<T> {
        match result {
            Ok(value) => Load::Ready(value),
            Err(reason) => Load::Failed(reason),
        }
    }

    fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(value) => Some(value),
            _ => None,
        }
    }
}

/// "New GitHub filter list" and "Edit filter list".
#[derive(Debug, Clone)]
pub struct Editor {
    target: Target,
    /// The GitHub account, `github:<login>`: where the preview runs and a new list goes.
    account: String,
    /// The accounts a new list can go to (chips when there are several); when editing, only the
    /// list's own.
    accounts: Vec<AccountChoice>,
    /// Opened from "New list…": the head has a back arrow.
    back: bool,
    name: String,
    /// The name was typed, or the list has one: no more suggestions ([`Editor::suggestion`]).
    named: bool,
    color: Option<String>,
    /// The searches, one to [`MAX_SEARCHES`].
    searches: Vec<String>,
    current: usize,
    /// The template the search was started from, until another filter changes it: names the list.
    template: Option<Template>,
    /// The repository chosen last, for any account; preselected in a new list.
    last_repo: Option<String>,
    /// The repositories the account reaches.
    repos: Load<Vec<GitHubRepo>>,
    /// Labels, milestones and people by repository in lower case, as asked for.
    details: BTreeMap<String, Load<GitHubRepoDetails>>,
    /// The list of repositories is open, with what was typed to find one.
    picker: Option<String>,
    /// The repository (lower case) just chosen: when its labels are known, labels and milestone it
    /// does not have leave the search.
    prune: Option<String>,
    /// "More filters" is open.
    more: bool,
    /// "GitHub search" is open.
    expert: bool,
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
    /// "New GitHub filter list" for the accounts of `start`; `None` without any. Starts with the
    /// search for open issues and no repository yet: [`Dialog::load`] asks for the repositories,
    /// and their answer preselects one.
    pub fn new_list(start: Start) -> Option<Dialog> {
        let chosen = start.accounts.iter().find(|a| a.id == start.account).or(start.accounts.first())?;
        let mut editor = Editor::blank(chosen.id.clone(), start.accounts.clone());
        editor.last_repo = start.last_repo;
        editor.back = start.back;
        Some(Dialog::Edit(Box::new(editor)))
    }

    /// "Edit filter…" of `list`, a filter list of `account` (shown as `account_label`); `None` for
    /// every other collection. "GitHub search" is open when the list has several searches or a
    /// search names several repositories. Its preview follows at once, after [`DEBOUNCE`];
    /// [`Dialog::load`] asks for the repositories and the labels of its repository.
    pub fn edit(list: &Collection, account: String, account_label: String) -> Option<Dialog> {
        let source = list.source.as_ref()?;
        let accounts = vec![AccountChoice { id: account.clone(), label: account_label }];
        let mut editor = Editor::blank(account, accounts);
        editor.target = Target::Edit { collection: list.id.clone(), revision: source.revision };
        editor.load(list);
        editor.touch(Instant::now());
        Some(Dialog::Edit(Box::new(editor)))
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

    /// The requests an editor needs when it opens: the repositories of its account
    /// (`tasks.list_github_repos`) and the labels, milestones and people of the repository its
    /// search names (`tasks.describe_github_repo`). The app runs the task right after opening;
    /// nothing for the question before deleting.
    pub fn load(&mut self, source: &Source) -> Task<Message> {
        match self {
            Dialog::Edit(editor) => {
                editor.repos = Load::Loading;
                Task::batch([editor.repos_task(source), editor.describe_task(source)])
            }
            Dialog::Delete(_) => Task::none(),
        }
    }

    /// Carries out `message`; requests go out through `source`. The task is the app's to run.
    pub fn update(&mut self, message: Message, source: &Source) -> (Task<Message>, Outcome) {
        match self {
            Dialog::Edit(editor) => editor.update(message, source),
            Dialog::Delete(delete) => delete.update(message, source),
        }
    }

    /// The repository to remember as the last one chosen (`ui.json`) once [`Outcome::Saved`] came:
    /// the one repository of the search being edited, as GitHub writes it; `None` when it names
    /// none or several.
    pub fn repo_to_remember(&self) -> Option<String> {
        match self {
            Dialog::Edit(editor) => editor.single_repo(),
            Dialog::Delete(_) => None,
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
    /// An editor of a new list on `account`: open issues, no name, the first list colour.
    fn blank(account: String, accounts: Vec<AccountChoice>) -> Editor {
        Editor {
            target: Target::New,
            account,
            accounts,
            back: false,
            name: String::new(),
            named: false,
            color: list_colors().into_iter().next(),
            searches: vec![NEW_SEARCH.to_string()],
            current: 0,
            template: None,
            last_repo: None,
            repos: Load::Loading,
            details: BTreeMap::new(),
            picker: None,
            prune: None,
            more: false,
            expert: false,
            preview: None,
            changed: None,
            asked: 0,
            loading: false,
            tried: false,
            saving: false,
            notice: None,
        }
    }

    /// Takes name, colour, searches and revision of `list`, and opens "GitHub search" and "More
    /// filters" where the form alone would hide something.
    fn load(&mut self, list: &Collection) {
        let Some(source) = list.source.as_ref() else { return };
        if let Target::Edit { revision, .. } = &mut self.target {
            *revision = source.revision;
        }
        self.name = list.name.clone();
        self.named = true;
        self.color = list.color.clone();
        self.searches = source.queries.iter().map(|q| q.trim().to_string()).collect();
        if self.searches.is_empty() {
            self.searches.push(String::new());
        }
        self.current = 0;
        self.template = None;
        self.expert = self.searches.len() > 1
            || self.searches.iter().any(|q| repos_of(q).len() > 1 || is_address(q))
            || self.searches.iter().all(|q| q.is_empty());
        self.more = has_more(self.query());
    }

    /// The search being edited.
    fn query(&self) -> &str {
        &self.searches[self.current.min(self.searches.len() - 1)]
    }

    fn query_mut(&mut self) -> &mut String {
        let at = self.current.min(self.searches.len() - 1);
        &mut self.searches[at]
    }

    /// The searches as they are sent: trimmed, empty ones left out.
    fn queries(&self) -> Vec<String> {
        self.searches.iter().map(|q| q.trim().to_string()).filter(|q| !q.is_empty()).collect()
    }

    /// The one repository of the search being edited, as the account's list writes it.
    fn single_repo(&self) -> Option<String> {
        let [repo]: [String; 1] = repos_of(self.query()).try_into().ok()?;
        let listed = self.repos.ready().and_then(|repos| repos.iter().find(|r| r.repo.eq_ignore_ascii_case(&repo)));
        Some(listed.map_or(repo, |r| r.repo.clone()))
    }

    /// What is known of the one repository of the search being edited.
    fn details(&self) -> Option<&Load<GitHubRepoDetails>> {
        self.details.get(&self.single_repo()?.to_lowercase())
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

    /// The name the list gets: the one typed, or the one suggested.
    fn current_name(&self) -> String {
        if self.named { self.name.clone() } else { self.suggestion() }
    }

    /// The name suggested from the filters until one is typed: what the search is about (the
    /// template, the labels, "Assigned to me", else type and state) and the repository's name –
    /// "Review requested · TorroWorkplace", "Bug · TorroWorkplace", "Open issues · notes".
    fn suggestion(&self) -> String {
        let query = self.query();
        let labels = field_values(query, Field::Labels);
        let about = match self.template {
            Some(template) => template.label().to_string(),
            None if !labels.is_empty() => capitalized(&labels.join(", ")),
            None if pick_of(query, Field::Assignee) == Pick::Me => tr("Assigned to me").to_string(),
            None => kind_and_state(Kind::of(query), State::of(query)).to_string(),
        };
        match repos_of(query).as_slice() {
            [repo] => format!("{about} · {}", repo.rsplit('/').next().unwrap_or(repo)),
            _ => about,
        }
    }

    /// Why the name or the searches cannot be saved, once Save was pressed.
    fn name_problem(&self) -> Option<&'static str> {
        (self.tried && self.current_name().trim().is_empty()).then(|| tr("Enter a name."))
    }

    fn query_problem(&self) -> Option<&'static str> {
        (self.tried && self.queries().is_empty()).then(|| tr("Enter a search or paste a github.com address."))
    }

    fn update(&mut self, message: Message, source: &Source) -> (Task<Message>, Outcome) {
        let task = match message {
            Message::Account(id) => self.switch_account(id, source),
            Message::Repos(account, result) => {
                if account == self.account {
                    self.repos = Load::of(result);
                    self.settle_repo();
                }
                Task::none()
            }
            Message::Described { account, repo, result } => {
                if account == self.account {
                    self.details.insert(repo, Load::of(result));
                    self.prune_now();
                }
                Task::none()
            }
            Message::RetryRepos => {
                self.repos = Load::Loading;
                self.repos_task(source)
            }
            Message::RetryDetails => {
                if let Some(repo) = self.single_repo() {
                    self.details.remove(&repo.to_lowercase());
                }
                Task::none()
            }
            Message::TogglePicker => {
                self.picker = match self.picker {
                    Some(_) => None,
                    None => Some(String::new()),
                };
                if self.picker.is_some() { operation::focus(REPO_INPUT) } else { Task::none() }
            }
            Message::FindRepo(text) => {
                self.picker = Some(text);
                Task::none()
            }
            Message::PickRepo(repo) => {
                self.picker = None;
                self.choose_repo(&repo);
                Task::none()
            }
            Message::PickFound => {
                if let Some(repo) = self.found().first().map(|r| r.repo.clone()) {
                    self.picker = None;
                    self.choose_repo(&repo);
                }
                Task::none()
            }
            Message::Template(template) => {
                // A template replaces the search but keeps its repository.
                let repos = repos_of(self.query());
                *self.query_mut() = set_values(template.query(), Field::Repos, &repos);
                self.template = Some(template);
                self.more = self.more || has_more(self.query());
                self.touch(Instant::now());
                Task::none()
            }
            Message::Kind(kind) => self.change(|query| kind.put(query)),
            Message::State(state) => self.change(|query| state.put(query)),
            Message::Sort(sort) => self.change(|query| sort.put(query)),
            Message::Label(name) => self.change(|query| toggle_label(query, Field::Labels, &name)),
            Message::Without(name) => self.change(|query| toggle_label(query, Field::WithoutLabels, &name)),
            Message::Pick(field, pick) => self.change(|query| set_pick(query, field, &pick)),
            Message::ToggleMore => {
                self.more = !self.more;
                Task::none()
            }
            Message::ToggleSearch => {
                self.expert = !self.expert;
                Task::none()
            }
            Message::Name(name) => {
                self.name = name;
                self.named = true;
                Task::none()
            }
            Message::Color(color) => {
                self.color = color;
                Task::none()
            }
            Message::Select(index) => {
                self.current = index.min(self.searches.len() - 1);
                self.picker = None;
                Task::none()
            }
            Message::AddSearch => {
                if self.searches.len() < MAX_SEARCHES {
                    self.searches.push(String::new());
                    self.current = self.searches.len() - 1;
                    self.expert = true;
                }
                Task::none()
            }
            Message::RemoveSearch(index) => {
                if self.searches.len() > 1 && index < self.searches.len() {
                    self.searches.remove(index);
                    self.current = self.current.min(self.searches.len() - 1);
                    self.touch(Instant::now());
                }
                Task::none()
            }
            Message::Query(query) => {
                *self.query_mut() = query;
                self.template = None;
                self.touch(Instant::now());
                Task::none()
            }
            Message::Tick(now) => self.preview_task(now, source),
            Message::Previewed(asked, result) => {
                // An answer to an older state of the searches is no use any more.
                if asked == self.asked {
                    self.loading = false;
                    self.preview = Some(result);
                }
                Task::none()
            }
            Message::Save => self.save(source),
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
                        (Task::none(), Outcome::Stay)
                    }
                };
            }
            // Back while the list is being saved would lose the answer: the arrow is off then.
            Message::Back if self.back && !self.saving => return (Task::none(), Outcome::Back),
            Message::Escape if self.picker.is_some() => {
                self.picker = None;
                Task::none()
            }
            Message::Escape | Message::Cancel => return (Task::none(), Outcome::Close),
            Message::Back | Message::Backdrop | Message::ConfirmDelete | Message::Deleted(_) => Task::none(),
        };
        let describe = self.describe_task(source);
        (Task::batch([task, describe]), Outcome::Stay)
    }

    /// Another account for a new list: its repositories are asked for; the repository stays when
    /// that account reaches it too ([`Editor::settle_repo`]).
    fn switch_account(&mut self, id: String, source: &Source) -> Task<Message> {
        if self.target != Target::New || id == self.account || !self.accounts.iter().any(|a| a.id == id) {
            return Task::none();
        }
        self.account = id;
        self.repos = Load::Loading;
        self.details.clear();
        self.picker = None;
        self.touch(Instant::now());
        self.repos_task(source)
    }

    /// Once the repositories of a new list are known: when its search names no repository, or
    /// one the account does not reach, the last one chosen takes its place – or, where the account
    /// does not reach that one either, the account's first. A search with several stays as it is.
    fn settle_repo(&mut self) {
        let Some(repos) = self.repos.ready().filter(|_| self.target == Target::New) else { return };
        let reached = |name: &str| repos.iter().find(|r| r.repo.eq_ignore_ascii_case(name)).map(|r| r.repo.clone());
        let named = repos_of(self.query());
        if named.len() > 1 || named.first().is_some_and(|repo| reached(repo).is_some()) {
            return;
        }
        let pick = self.last_repo.as_deref().and_then(reached).or_else(|| repos.first().map(|r| r.repo.clone()));
        if let Some(repo) = pick {
            self.choose_repo(&repo);
        }
    }

    /// `repo` as the one repository of the search being edited; labels and milestone it does not
    /// have leave the search as soon as its own are known.
    fn choose_repo(&mut self, repo: &str) {
        let query = set_values(self.query(), Field::Repos, &[repo.to_string()]);
        if query != self.query() {
            *self.query_mut() = query;
            self.prune = Some(repo.to_lowercase());
            self.prune_now();
            self.touch(Instant::now());
        }
    }

    /// Takes labels and milestone the repository just chosen does not have out of the search,
    /// once its labels are known.
    fn prune_now(&mut self) {
        let Some(repo) = self.prune.clone() else { return };
        let Some(Load::Ready(details)) = self.details.get(&repo) else { return };
        let mut query = self.query().to_string();
        for side in [Field::Labels, Field::WithoutLabels] {
            let kept: Vec<String> = field_values(&query, side)
                .into_iter()
                .filter(|name| details.labels.iter().any(|l| l.name.eq_ignore_ascii_case(name)))
                .collect();
            query = set_values(&query, side, &kept);
        }
        if let Pick::Value(title) = pick_of(&query, Field::Milestone)
            && !details.milestones.iter().any(|m| m.eq_ignore_ascii_case(&title))
        {
            query = set_pick(&query, Field::Milestone, &Pick::Any);
        }
        self.prune = None;
        if query != self.query() {
            *self.query_mut() = query;
            self.touch(Instant::now());
        }
    }

    /// The repositories the list of the repository field shows: those whose name has what was
    /// typed, in the order the core gave them (the most recently changed first).
    fn found(&self) -> Vec<&GitHubRepo> {
        let typed = self.picker.as_deref().unwrap_or_default().trim().to_lowercase();
        let Some(repos) = self.repos.ready() else { return Vec::new() };
        repos.iter().filter(|r| r.repo.to_lowercase().contains(&typed)).collect()
    }

    /// Rewrites the search being edited with `change` (a choice of the form); a preview follows
    /// when the search changed. The template it was started from no longer names it.
    fn change(&mut self, change: impl FnOnce(&str) -> String) -> Task<Message> {
        let query = change(self.query());
        if query != self.query() {
            *self.query_mut() = query;
            self.template = None;
            self.touch(Instant::now());
        }
        Task::none()
    }

    /// Asks the core for the repositories of the account.
    fn repos_task(&self, source: &Source) -> Task<Message> {
        let (source, account) = (source.clone(), self.account.clone());
        let asked = account.clone();
        Task::perform(
            async move { source.list_github_repos(account).await.map_err(|err| problem_text(&err)) },
            move |result| Message::Repos(asked, result),
        )
    }

    /// Asks the core for labels, milestones and people of the one repository of the search being
    /// edited, unless they are known or on their way.
    fn describe_task(&mut self, source: &Source) -> Task<Message> {
        if is_address(self.query()) {
            return Task::none();
        }
        let Some(repo) = self.single_repo() else { return Task::none() };
        let key = repo.to_lowercase();
        if self.details.contains_key(&key) {
            return Task::none();
        }
        self.details.insert(key.clone(), Load::Loading);
        let (source, account) = (source.clone(), self.account.clone());
        let asked = account.clone();
        Task::perform(
            async move { source.describe_github_repo(account, repo).await.map_err(|err| problem_text(&err)) },
            move |result| Message::Described { account: asked, repo: key, result },
        )
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
            self.expert = true;
            return operation::focus(QUERY_INPUT);
        }
        if self.saving {
            return Task::none();
        }
        self.saving = true;
        self.picker = None;
        if self.notice != Some(Notice::Reloaded) {
            self.notice = None;
        }
        let name = self.current_name().trim().to_string();
        let definition = FilterDefinition { name, color: self.color.clone(), queries: self.queries() };
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
            Target::New => tr("New GitHub filter list"),
            Target::Edit { .. } => tr("Edit filter list"),
        }
    }

    // --- view ---

    /// Over the window: the card on the dialog surface, [`WIDTH`] wide with the preview as a
    /// column at the right where the window has room for it, else at most [`size::FORM_MAX`] with
    /// the preview under the form; at most [`dialog_height`] high, the form scrolling.
    fn dialog(&self, window: Size) -> Element<'_, Message> {
        let beside = window.width >= WIDTH + 2.0 * space::L;
        let back = (self.back && !self.saving).then_some(Message::Back);
        let mut head = row![].spacing(space::XS).align_y(Alignment::Center).padding(space::M);
        if self.back {
            head = head.push(widgets::icon_button(Icon::ChevronLeft, tr("Back"), back));
        }
        let head = head
            .push(container(typo::text(typo::TITLE, self.title())).padding([0.0, space::S]).width(Fill))
            .push(widgets::icon_button(Icon::X, tr("Close"), Some(Message::Cancel)));
        let middle: Element<'_, Message> = if beside {
            row![
                self.scrolled(space::XL, false, None),
                rule::vertical(LINE).style(style::rule),
                container(self.preview_view(size::PROPS - 2.0 * space::L, true))
                    .width(size::PROPS)
                    .height(Fill)
                    .padding(space::L)
                    .style(raised),
            ]
            .height(Fill)
            .into()
        } else {
            let room = (window.width - 2.0 * space::L).min(size::FORM_MAX) - 2.0 * space::XL;
            self.scrolled(space::XL, false, Some(room))
        };
        let card = column![
            head,
            rule::horizontal(LINE).style(style::rule),
            middle,
            rule::horizontal(LINE).style(style::rule),
            container(self.footer(!beside)).padding([space::L, space::XL]),
        ];
        let card = container(card).width(Fill).height(dialog_height(window.height)).style(style::container::popover);
        let width = if beside { WIDTH } else { size::FORM_MAX };
        widgets::modal(Space::new().width(Fill).height(Fill), card, width, Message::Backdrop)
    }

    /// In a narrow window: a view of its own over the whole window, with a header bar ("‹ New
    /// GitHub filter list ✕"), the form and the preview under it scrolling, the buttons at the
    /// bottom.
    fn full(&self, width: f32) -> Element<'_, Message> {
        let close = widgets::icon_button(Icon::X, tr("Close"), Some(Message::Cancel));
        let head = if self.back {
            let back = widgets::icon_button(Icon::ChevronLeft, tr("Back"), (!self.saving).then_some(Message::Back));
            torro_gui::shell::header_bar([back], torro_gui::shell::header_title(None, self.title(), None), [close])
        } else {
            torro_gui::shell::header_bar([close], torro_gui::shell::header_title(None, self.title(), None), [])
        };
        let head = container(head).center_y(size::HEADER).padding([0.0, space::S]);
        let view = column![
            head,
            rule::horizontal(LINE).style(style::rule),
            self.scrolled(space::L, true, Some(width - 2.0 * space::L)),
            rule::horizontal(LINE).style(style::rule),
            container(self.footer(true)).padding([space::M, space::L]),
        ];
        container(view).width(Fill).height(Fill).style(style::container::panel_flush).into()
    }

    /// The form, scrolling, with `inset` around it; the preview under it when `preview` gives the
    /// width of the form (the preview is not beside it).
    fn scrolled(&self, inset: f32, narrow: bool, preview: Option<f32>) -> Element<'_, Message> {
        let mut body = column![self.body(narrow)].spacing(space::XL);
        if let Some(width) = preview {
            body = body.push(rule::horizontal(LINE).style(style::rule)).push(self.preview_view(width, false));
        }
        scrollable(container(body).padding(inset).width(Fill))
            .id(FORM_SCROLL)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// Notice, account, repository, the filters, name and colour, and the search they write.
    fn body(&self, narrow: bool) -> Element<'_, Message> {
        let mut parts: Vec<Element<'_, Message>> = Vec::new();
        if let Some(notice) = self.notice_view() {
            parts.push(notice);
        }
        if self.target == Target::New && self.accounts.len() > 1 {
            let chips = self.accounts.iter().map(|account| {
                let on = account.id == self.account;
                widgets::choice_chip(account.label.as_str(), None, on, Some(Message::Account(account.id.clone())))
            });
            parts.push(labelled(tr("Account"), widgets::chip_row(chips.collect()), None).into());
        }
        parts.push(self.repo_view());
        if is_address(self.query()) {
            parts.push(hint(
                Icon::Link,
                tr("This is an address on github.com. The list turns it into a search when you save."),
            ));
        } else {
            parts.push(self.templates());
            parts.push(self.kind_and_state(narrow));
            parts.push(self.labels_view());
            parts.push(self.assignee_view());
            parts.push(self.milestone_view());
            parts.push(self.more_view(narrow));
        }
        parts.push(rule::horizontal(LINE).style(style::rule).into());
        if narrow {
            parts.push(column![self.name_field(), self.colors()].spacing(space::L).into());
        } else {
            parts.push(row![self.name_field(), self.colors()].spacing(space::L).align_y(Alignment::Start).into());
        }
        parts.push(self.search_view());
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

    /// "Repository": the field that opens the list of repositories, what it is for, the list when
    /// it is open, and why the repositories are missing, with "Retry".
    fn repo_view(&self) -> Element<'_, Message> {
        let (label, quiet, several) = self.repo_label();
        let repo = self.single_repo();
        let name = typo::text(if quiet { typo::BODY } else { typo::BODY_STRONG }, label).wrapping(Wrapping::None);
        let name = if quiet { name.style(theme::text_3) } else { name };
        let mut content = row![list::toned(Icon::Github, icon::SIZE_SM, Tone::Text3), name]
            .spacing(space::S)
            .align_y(Alignment::Center);
        let last = repo.as_ref().zip(self.last_repo.as_ref()).is_some_and(|(a, b)| a.eq_ignore_ascii_case(b));
        if last && !several {
            content = content.push(widgets::icon_pill(Icon::Clock, tr("Last used")));
        }
        let chevron = if self.picker.is_some() { Icon::ChevronUp } else { Icon::ChevronDown };
        content = content.push(Space::new().width(Fill)).push(list::toned(chevron, icon::SIZE_SM, Tone::Text3));
        let field = button(container(content).center_y(Fill).clip(true))
            .width(Fill)
            .height(size::CONTROL)
            .padding(style::ITEM_PADDING)
            .style(style::button::secondary)
            .on_press(Message::TogglePicker);
        let mut parts: Vec<Element<'_, Message>> = vec![field.into()];
        if self.picker.is_some() {
            parts.push(self.picker_view());
        } else if let Load::Failed(reason) = &self.repos {
            parts.push(retry(reason.as_str(), Message::RetryRepos));
        } else {
            parts.push(
                typo::text(
                    typo::META,
                    tr("Repositories your GitHub account can reach. Labels, milestones and people below come from here."),
                )
                .style(theme::text_2)
                .into(),
            );
        }
        labelled(tr("Repository"), column(parts).spacing(space::XS), None).into()
    }

    /// What the repository field says, whether quietly (nothing chosen), and whether the list
    /// searches in several repositories: "Several repositories" with several searches or several
    /// repositories in the search, else the one repository; without one "Loading repositories…",
    /// "All repositories" for a list whose search names none (it searches all the account sees),
    /// else "Choose a repository".
    fn repo_label(&self) -> (String, bool, bool) {
        let several = self.searches.len() > 1 || repos_of(self.query()).len() > 1;
        match self.single_repo() {
            _ if several => (tr("Several repositories").into(), false, true),
            Some(repo) => (repo, false, false),
            None if self.repos == Load::Loading => (tr("Loading repositories…").into(), true, false),
            None if self.target != Target::New => (tr("All repositories").into(), true, false),
            None => (tr("Choose a repository").into(), true, false),
        }
    }

    /// The open list of repositories: a field to find one (Enter takes the first found), then at
    /// most [`REPO_ROWS`] of them, the chosen one with a check, the last chosen and private ones
    /// with a word at the end.
    fn picker_view(&self) -> Element<'_, Message> {
        let typed = self.picker.as_deref().unwrap_or_default();
        let find = field(tr("Find a repository…"), typed)
            .id(REPO_INPUT)
            .on_input(Message::FindRepo)
            .on_submit(Message::PickFound);
        let mut rows: Vec<Element<'_, Message>> = vec![find.into()];
        match &self.repos {
            Load::Loading => rows.push(container(quiet(tr("Loading repositories…"))).padding(space::S).into()),
            Load::Failed(reason) => {
                rows.push(container(retry(reason.as_str(), Message::RetryRepos)).padding(space::S).into())
            }
            Load::Ready(_) => {
                let found = self.found();
                let chosen = self.single_repo().filter(|_| repos_of(self.query()).len() == 1);
                for repo in found.iter().take(REPO_ROWS) {
                    let on = chosen.as_ref().is_some_and(|c| c.eq_ignore_ascii_case(&repo.repo));
                    rows.push(self.repo_row(repo, on));
                }
                if found.is_empty() {
                    rows.push(container(quiet(tr("No repository matches."))).padding(space::S).into());
                } else if found.len() > REPO_ROWS {
                    let more = trf("{n} more. Type to narrow the list.", &[("n", &(found.len() - REPO_ROWS))]);
                    rows.push(container(typo::text(typo::META, more).style(theme::text_3)).padding(space::S).into());
                }
            }
        }
        container(column(rows).spacing(space::XS))
            .padding(style::MENU_INSET)
            .width(Fill)
            .style(style::container::menu)
            .into()
    }

    /// One repository in the open list: check (when `on`), name, "Last used" / "Private".
    fn repo_row<'a>(&'a self, repo: &'a GitHubRepo, on: bool) -> Element<'a, Message> {
        let check: Element<'a, Message> = if on {
            list::toned(Icon::Check, icon::SIZE_SM, Tone::Text3)
        } else {
            Space::new().width(icon::SIZE_SM).into()
        };
        let mut content = row![
            check,
            container(typo::text(typo::BODY, repo.repo.as_str()).wrapping(Wrapping::None)).width(Fill).clip(true)
        ]
        .spacing(space::S)
        .align_y(Alignment::Center);
        if self.last_repo.as_deref().is_some_and(|last| last.eq_ignore_ascii_case(&repo.repo)) {
            content = content.push(typo::text(typo::META, tr("Last used")).style(theme::text_3));
        }
        if repo.private {
            content = content.push(typo::text(typo::META, tr("Private")).style(theme::text_3));
        }
        button(container(content).center_y(Fill))
            .width(Fill)
            .height(style::MENU_ITEM_HEIGHT)
            .padding(style::ITEM_PADDING)
            .style(style::button::menu_item(on))
            .on_press(Message::PickRepo(repo.repo.clone()))
            .into()
    }

    /// "Start from": the templates, the one the search came from chosen.
    fn templates(&self) -> Element<'_, Message> {
        let chips = Template::ALL.iter().map(|&t| {
            widgets::choice_chip(
                t.label(),
                Some(Mark::Icon(t.icon())),
                self.template == Some(t),
                Some(Message::Template(t)),
            )
        });
        labelled(tr("Start from"), widgets::chip_row(chips.collect()), None).into()
    }

    /// "Show" (issues, pull requests, both) and "State" as segments, side by side.
    fn kind_and_state(&self, narrow: bool) -> Element<'_, Message> {
        let query = self.query();
        let kinds = [(tr("Issues"), Kind::Issues), (tr("Pull requests"), Kind::PullRequests), (tr("Both"), Kind::Both)];
        let kind = Kind::of(query);
        let kind_at = kinds.iter().position(|(_, k)| *k == kind).unwrap_or_default();
        let kind = widgets::segmented(kinds.map(|(label, k)| (label, Message::Kind(k))), kind_at);
        let states = [(tr("Open"), State::Open), (tr("Closed"), State::Closed), (tr("Any"), State::Any)];
        let state = State::of(query);
        let state_at = states.iter().position(|(_, s)| *s == state).unwrap_or_default();
        let state = widgets::segmented(states.map(|(label, s)| (label, Message::State(s))), state_at);
        let (kind, state) = (labelled(tr("Show"), kind, None), labelled(tr("State"), state, None));
        if narrow { column![kind, state].spacing(space::L).into() } else { row![kind, state].spacing(space::XL).into() }
    }

    /// The label chips of one side ([`Field::Labels`] or [`Field::WithoutLabels`]): the
    /// repository's labels and those of the search it does not have, the chosen ones checked.
    /// While the labels load, quiet placeholders; when they could not be read, why, with "Retry".
    fn label_chips(&self, side: Field) -> Element<'_, Message> {
        let chosen = field_values(self.query(), side);
        let is_on = |name: &str| chosen.iter().any(|c| c.eq_ignore_ascii_case(name));
        let message = |name: String| if side == Field::Labels { Message::Label(name) } else { Message::Without(name) };
        let known: &[GitHubLabel] = match self.details() {
            Some(Load::Ready(details)) => &details.labels,
            _ => &[],
        };
        let mut chips: Vec<Element<'_, Message>> = known
            .iter()
            .map(|label| {
                let hex = label.color.as_deref().unwrap_or_default();
                widgets::label_choice(hex, label.name.as_str(), is_on(&label.name), Some(message(label.name.clone())))
            })
            .collect();
        for name in chosen.iter().filter(|name| !known.iter().any(|l| l.name.eq_ignore_ascii_case(name))) {
            chips.push(widgets::label_choice("", name.clone(), true, Some(message(name.clone()))));
        }
        let below: Option<Element<'_, Message>> = match self.details() {
            Some(Load::Loading) => Some(placeholders()),
            Some(Load::Failed(reason)) => Some(retry(reason.as_str(), Message::RetryDetails)),
            Some(Load::Ready(_)) => None,
            None => Some(quiet_meta(tr("Choose one repository to pick from its labels."))),
        };
        let mut parts: Vec<Element<'_, Message>> = Vec::new();
        if !chips.is_empty() {
            parts.push(widgets::chip_row(chips));
        }
        parts.extend(below);
        column(parts).spacing(space::S).into()
    }

    /// "Labels": the chips and, where there are chips, what they mean together.
    fn labels_view(&self) -> Element<'_, Message> {
        let mut content = column![self.label_chips(Field::Labels)].spacing(space::S);
        let chips =
            !field_values(self.query(), Field::Labels).is_empty() || matches!(self.details(), Some(Load::Ready(_)));
        if chips {
            content = content
                .push(typo::text(typo::META, tr("Shows items that have all selected labels.")).style(theme::text_2));
        }
        labelled(Field::Labels.label(), content, None).into()
    }

    /// "Assignee": anyone, me, unassigned and the people of the repository (the account itself is
    /// "Me"), plus the one of the search when it is none of them.
    fn assignee_view(&self) -> Element<'_, Message> {
        let mut picks = vec![(tr("Anyone").to_string(), Pick::Any), (tr("Me").to_string(), Pick::Me)];
        picks.push((tr("Unassigned").to_string(), Pick::Nothing));
        let me = self.account.strip_prefix("github:").unwrap_or(&self.account);
        if let Some(Load::Ready(details)) = self.details() {
            let people = details.assignees.iter().filter(|login| !login.eq_ignore_ascii_case(me));
            picks.extend(people.map(|login| (login.clone(), Pick::Value(login.clone()))));
        }
        self.pick_chips(Field::Assignee, picks)
    }

    /// "Milestone": any, the open milestones of the repository, none – plus the one of the search
    /// when it is none of them.
    fn milestone_view(&self) -> Element<'_, Message> {
        let mut picks = vec![(tr("Any").to_string(), Pick::Any)];
        if let Some(Load::Ready(details)) = self.details() {
            picks.extend(details.milestones.iter().map(|title| (title.clone(), Pick::Value(title.clone()))));
        }
        picks.push((tr("No milestone").to_string(), Pick::Nothing));
        self.pick_chips(Field::Milestone, picks)
    }

    /// The chips of a filter of one value under its label, the one of the search chosen; a value
    /// of the search that `picks` does not have is added, so it shows and can be undone.
    fn pick_chips(&self, field: Field, mut picks: Vec<(String, Pick)>) -> Element<'_, Message> {
        let current = pick_of(self.query(), field);
        let listed = |pick: &Pick| {
            picks.iter().any(|(_, p)| match (p, pick) {
                (Pick::Value(a), Pick::Value(b)) => a.eq_ignore_ascii_case(b),
                (a, b) => a == b,
            })
        };
        if let Pick::Value(value) = &current
            && !listed(&current)
        {
            picks.push((value.clone(), current.clone()));
        }
        let chips = picks.into_iter().map(|(label, pick)| {
            let on = match (&pick, &current) {
                (Pick::Value(a), Pick::Value(b)) => a.eq_ignore_ascii_case(b),
                (a, b) => a == b,
            };
            widgets::choice_chip(label, None, on, Some(Message::Pick(field, pick)))
        });
        labelled(field.label(), widgets::chip_row(chips.collect()), None).into()
    }

    /// "More filters" with what of it is set; open: author, review requested, mentioned, sort and
    /// the labels an item must not have.
    fn more_view(&self, narrow: bool) -> Element<'_, Message> {
        let query = self.query();
        let mut set: Vec<&str> = Vec::new();
        for field in [Field::Author, Field::ReviewRequested, Field::Mentions] {
            if pick_of(query, field) != Pick::Any {
                set.push(field.label());
            }
        }
        if Sort::of(query) != Sort::BestMatch {
            set.push(tr("Sort"));
        }
        if !field_values(query, Field::WithoutLabels).is_empty() {
            set.push(Field::WithoutLabels.label());
        }
        let mut head = row![disclosure(tr("More filters"), self.more, Message::ToggleMore)]
            .spacing(space::XS)
            .align_y(Alignment::Center);
        if !set.is_empty() {
            head = head.push(typo::text(typo::META, format!("· {}", set.join(", "))).style(theme::text_3));
        }
        if !self.more {
            return head.into();
        }
        let sort = Sort::of(query);
        let sorts = Sort::choices(&sort);
        let sort_at = sorts.iter().position(|s| *s == sort).unwrap_or_default();
        let sort = widgets::segmented(sorts.into_iter().map(|s| (s.to_string(), Message::Sort(s))), sort_at);
        let people = [
            self.person_segments(Field::Author),
            self.person_segments(Field::ReviewRequested),
            self.person_segments(Field::Mentions),
        ];
        let people: Element<'_, Message> =
            if narrow { column(people).spacing(space::L).into() } else { row(people).spacing(space::XL).into() };
        let content = column![
            people,
            labelled(tr("Sort"), sort, None),
            labelled(Field::WithoutLabels.label(), self.label_chips(Field::WithoutLabels), None),
        ]
        .spacing(space::L);
        column![head, container(content).padding(Padding::ZERO.left(space::XL))].spacing(space::M).into()
    }

    /// Anyone / Me (and a login of the search) as segments for `field`.
    fn person_segments(&self, field: Field) -> Element<'_, Message> {
        let current = pick_of(self.query(), field);
        let mut picks = vec![(tr("Anyone").to_string(), Pick::Any), (tr("Me").to_string(), Pick::Me)];
        if let Pick::Value(login) = &current {
            picks.push((login.clone(), current.clone()));
        }
        let at = picks.iter().position(|(_, pick)| *pick == current).unwrap_or_default();
        let segments =
            widgets::segmented(picks.into_iter().map(|(label, pick)| (label, Message::Pick(field, pick))), at);
        labelled(field.label(), segments, None).into()
    }

    /// The name, suggested from the filters until one is typed.
    fn name_field(&self) -> Element<'_, Message> {
        let input = field(tr("Name of the list"), &self.current_name())
            .id(NAME_INPUT)
            .on_input(Message::Name)
            .on_submit(Message::Save);
        let mut parts = labelled(tr("Name"), input, self.name_problem());
        if !self.named {
            parts = parts.push(
                typo::text(typo::META, tr("Suggested from your filters. Type to change it.")).style(theme::text_3),
            );
        }
        parts.width(Fill).into()
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

    /// "GitHub search": what the filters write, open: the searches of the list (as rows when there
    /// are several), the search being edited as a field to change by hand or paste into, and
    /// "Add another search".
    fn search_view(&self) -> Element<'_, Message> {
        let head = disclosure(tr("GitHub search"), self.expert, Message::ToggleSearch);
        if !self.expert {
            return head;
        }
        let mut parts: Vec<Element<'_, Message>> = Vec::new();
        if self.searches.len() > 1 {
            parts.push(self.search_rows());
        }
        let query = field(tr("is:open is:issue repo:owner/name label:bug"), self.query())
            .id(QUERY_INPUT)
            .on_input(Message::Query)
            .on_submit(Message::Save);
        parts.push(labelled(tr("Search"), query, self.query_problem()).into());
        parts.push(
            typo::text(
                typo::META,
                tr("The filters above write this search. Edit it by hand, or paste a search or a github.com address to start from it; words the form does not know stay as they are."),
            )
            .style(theme::text_2)
            .into(),
        );
        let add = (self.searches.len() < MAX_SEARCHES).then_some(Message::AddSearch);
        parts.push(widgets::text_button(Some(Icon::Plus), tr("Add another search"), style::button::ghost, add));
        let content = container(column(parts).spacing(space::M)).padding(Padding::ZERO.left(space::XL));
        column![head, content].spacing(space::M).into()
    }

    /// One row per search, the one being edited marked; "✕" removes one.
    fn search_rows(&self) -> Element<'_, Message> {
        let rows = self.searches.iter().enumerate().map(|(index, query)| {
            let label: Element<'_, Message> = if query.is_empty() {
                typo::text(typo::BODY, tr("Empty search")).style(theme::text_3).wrapping(Wrapping::None).into()
            } else {
                typo::text(typo::BODY, query.as_str()).wrapping(Wrapping::None).into()
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

    /// "Preview": "Checking…" while the core is asked, the number of matches, the first ones in a
    /// column `width` wide – in the narrow column beside the form (`beside`) with the title over
    /// `repo#123`, else on one line each –, the empty state with advice, or what went wrong.
    fn preview_view(&self, width: f32, beside: bool) -> Element<'_, Message> {
        let mut head = row![typo::text(typo::CAPTION, tr("Preview")).style(theme::text_3).width(Fill)]
            .spacing(space::XS)
            .align_y(Alignment::Center);
        if self.loading {
            head = head.push(list::toned(Icon::RefreshCw, icon::SIZE_SM, Tone::Text3));
            head = head.push(typo::text(typo::META, tr("Checking…")).style(theme::text_3));
        }
        let mut parts: Vec<Element<'_, Message>> = vec![head.into()];
        match &self.preview {
            None if self.loading => {}
            None => parts.push(quiet(tr("The first matches show here once there is a search."))),
            Some(Err(reason)) => parts.push(hint(Icon::CircleAlert, reason.clone())),
            Some(Ok(preview)) => {
                parts.push(typo::text(typo::HEADLINE.tabular(), matches_text(preview.matches)).into());
                if preview.truncated {
                    parts.push(quiet_meta(tr("The list shows the first 1,000.")));
                }
                if preview.todos.is_empty() {
                    parts.push(
                        column![
                            typo::text(typo::BODY, tr("Nothing matches.")).style(theme::text_2),
                            typo::text(
                                typo::META,
                                tr("Try fewer labels or another state. The list can still be created; it fills when something matches."),
                            )
                            .style(theme::text_3),
                        ]
                        .spacing(space::XS)
                        .into(),
                    );
                } else {
                    let first = preview.todos.iter().take(PREVIEW_ROWS);
                    let rows: Vec<Element<'_, Message>> = if beside {
                        first.map(match_block).collect()
                    } else {
                        first.map(|todo| match_row(todo, width)).collect()
                    };
                    let spacing = if beside { space::M } else { space::XS };
                    parts.push(column(rows).spacing(spacing).into());
                }
            }
        }
        parts.push(Space::new().height(Fill).into());
        parts.push(quiet_meta(tr("Updates as you choose. Pinned issues can be added later.")));
        column(parts).spacing(space::M).into()
    }

    /// "Cancel" and the one primary button ("Create list" / "Save", "Creating…" / "Saving…" while
    /// the core is asked); at the left the number of matches when the preview is not beside the
    /// form (`count`).
    fn footer(&self, count: bool) -> Element<'_, Message> {
        let status: Element<'_, Message> = match &self.preview {
            _ if !count => Space::new().into(),
            _ if self.loading && self.preview.is_none() => quiet(tr("Checking…")),
            Some(Ok(preview)) => {
                typo::text(typo::META.tabular(), matches_text(preview.matches)).style(theme::text_2).into()
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
        let (label, busy) = match self.target {
            Target::New => (tr("Create list"), tr("Creating…")),
            Target::Edit { .. } => (tr("Save"), tr("Saving…")),
        };
        let save = if self.saving {
            widgets::busy_button(busy, style::button::primary)
        } else {
            widgets::text_button(None, label, style::button::primary, Some(Message::Save))
        };
        row![
            container(status).width(Fill).clip(true),
            widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Cancel)),
            save,
        ]
        .spacing(space::S)
        .align_y(Alignment::Center)
        .into()
    }
}

/// Whether "More filters" has something set in `query`: author, review requested, mentioned, sort,
/// labels an item must not have.
fn has_more(query: &str) -> bool {
    [Field::Author, Field::ReviewRequested, Field::Mentions].iter().any(|&field| pick_of(query, field) != Pick::Any)
        || Sort::of(query) != Sort::BestMatch
        || !field_values(query, Field::WithoutLabels).is_empty()
}

/// What a search of `kind` in `state` finds, as the start of a suggested name.
fn kind_and_state(kind: Kind, state: State) -> &'static str {
    match (kind, state) {
        (Kind::Issues, State::Open) => tr("Open issues"),
        (Kind::Issues, State::Closed) => tr("Closed issues"),
        (Kind::Issues, State::Any) => tr("Issues"),
        (Kind::PullRequests, State::Open) => tr("Open pull requests"),
        (Kind::PullRequests, State::Closed) => tr("Closed pull requests"),
        (Kind::PullRequests, State::Any) => tr("Pull requests"),
        (Kind::Both, State::Open) => tr("Open issues and pull requests"),
        (Kind::Both, State::Closed) => tr("Closed issues and pull requests"),
        (Kind::Both, State::Any) => tr("Issues and pull requests"),
    }
}

/// `text` with its first letter in upper case ("good first issue" → "Good first issue").
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

impl Delete {
    fn update(&mut self, message: Message, source: &Source) -> (Task<Message>, Outcome) {
        match message {
            Message::Cancel | Message::Escape => (Task::none(), Outcome::Close),
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

/// A part that opens and closes ("› More filters"): chevron and label in `CAPTION`, on the left
/// edge of the form like the labels above it, ghost states around it.
fn disclosure<'a>(label: &'a str, open: bool, toggle: Message) -> Element<'a, Message> {
    let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
    let content = row![list::toned(chevron, icon::SIZE_SM, Tone::Text3), typo::text(typo::CAPTION, label)]
        .spacing(space::XS)
        .align_y(Alignment::Center);
    button(container(content).center_y(Fill))
        .height(size::CONTROL_SM)
        .padding(0)
        .style(style::button::ghost)
        .on_press(toggle)
        .into()
}

/// A quiet small line (`META`, `text_3`).
fn quiet_meta<'a>(line: &'a str) -> Element<'a, Message> {
    typo::text(typo::META, line).style(theme::text_3).into()
}

/// What could not be read from GitHub, with "Retry" that sends `again`.
fn retry<'a>(reason: &'a str, again: Message) -> Element<'a, Message> {
    row![
        list::toned(Icon::CircleAlert, icon::SIZE_SM, Tone::Attention),
        typo::text(typo::META, reason).style(theme::text_2).width(Fill),
        widgets::text_button(None, tr("Retry"), style::button::ghost, Some(again)),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center)
    .into()
}

/// Quiet chip-shaped placeholders while the labels of a repository load: no text, nothing moves.
fn placeholders<'a>() -> Element<'a, Message> {
    let chips = PLACEHOLDERS.map(|width| {
        container(Space::new()).width(width).height(size::CONTROL_SM).style(style::container::chip).into()
    });
    widgets::chip_row(chips.into())
}

/// The surface of the preview column: `raised`, like the side of a dialog.
fn raised(theme: &Theme) -> container::Style {
    container::Style { background: Some(Background::Color(palette(theme).raised)), ..container::Style::default() }
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

/// A match in the preview column beside the form: its type symbol, the title (wrapping) and
/// `repo#123` under it.
fn match_block(todo: &Todo) -> Element<'_, Message> {
    let mark: Element<'_, Message> = match list::github_of(todo) {
        Some(details) => {
            let (which, tone) = list::github_mark(details);
            list::toned(which, icon::SIZE_SM, tone)
        }
        None => list::toned(Icon::CircleDot, icon::SIZE_SM, Tone::Text3),
    };
    let reference = todo
        .link
        .as_ref()
        .and_then(|link| link.github_ref())
        .map(|(repo, number)| format!("{}#{number}", repo.rsplit('/').next().unwrap_or(repo)))
        .unwrap_or_default();
    let text = column![
        typo::text(typo::BODY, todo.summary.as_str()),
        typo::text(typo::META.tabular(), reference).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::XS);
    // The symbol sits on the first line of the title.
    row![container(mark).center_y(typo::line_height(typo::BODY)), text]
        .spacing(space::S)
        .align_y(Alignment::Start)
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
    use iced::futures::executor::block_on;
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

    const TORRO: &str = "mahype/TorroWorkplace";
    const DESIGN: &str = "mahype/torro-design";

    fn account(id: &str) -> AccountChoice {
        AccountChoice { id: id.into(), label: id.into() }
    }

    fn start(last_repo: Option<&str>) -> Start {
        Start {
            accounts: vec![account(GITHUB_ACCOUNT)],
            account: GITHUB_ACCOUNT.into(),
            last_repo: last_repo.map(Into::into),
            back: true,
        }
    }

    fn new_dialog() -> Dialog {
        Dialog::new_list(start(None)).expect("a GitHub account")
    }

    /// The demo's answer to `tasks.list_github_repos`, handed to the dialog.
    fn repos_arrive(dialog: &mut Dialog) {
        let repos = block_on(demo().list_github_repos(GITHUB_ACCOUNT.into())).map_err(|err| err.to_string());
        let _ = dialog.update(Message::Repos(GITHUB_ACCOUNT.into(), repos), &demo());
    }

    /// The demo's answer to `tasks.describe_github_repo` for `repo`, handed to the dialog.
    fn described(dialog: &mut Dialog, repo: &str) {
        let result =
            block_on(demo().describe_github_repo(GITHUB_ACCOUNT.into(), repo.into())).map_err(|e| e.to_string());
        let message = Message::Described { account: GITHUB_ACCOUNT.into(), repo: repo.to_lowercase(), result };
        let _ = dialog.update(message, &demo());
    }

    /// A new list on the demo account with `repo` chosen and its labels known.
    fn new_on(repo: &str) -> Dialog {
        let mut dialog = new_dialog();
        repos_arrive(&mut dialog);
        let _ = dialog.update(Message::PickRepo(repo.into()), &demo());
        described(&mut dialog, repo);
        dialog
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
        assert_eq!((e.query(), e.current_name()), (Template::ReviewRequested.query(), tr("Review requested").into()));
        assert_eq!(pick_of(e.query(), Field::ReviewRequested), Pick::Me);
        assert!(e.more, "review requested lives under More filters, which opens");
        // A template keeps the repository and names the list after both.
        let mut dialog = new_on(TORRO);
        let _ = dialog.update(Message::Template(Template::AssignedToMe), &demo());
        let e = editor(&dialog);
        assert_eq!(e.query(), format!("{} repo:{TORRO}", Template::AssignedToMe.query()));
        assert_eq!(e.current_name(), format!("{} · TorroWorkplace", tr("Assigned to me")));
        // A name of one's own stays.
        let mut dialog = new_dialog();
        let _ = dialog.update(Message::Name("Mine".into()), &demo());
        let _ = dialog.update(Message::Template(Template::Mentioned), &demo());
        assert_eq!(editor(&dialog).current_name(), "Mine");
    }

    #[test]
    fn an_address_becomes_the_search_as_it_is() {
        let address = "https://github.com/mahype/TorroWorkplace/issues?q=is%3Aopen+label%3Abug";
        assert!(is_address(address) && is_address("github.com/issues") && !is_address("is:open repo:a/b"));
        let mut dialog = new_dialog();
        let _ = dialog.update(Message::Query(address.into()), &demo());
        let e = editor(&dialog);
        assert_eq!(e.query(), address, "the core turns it into a search");
        assert!(e.changed.is_some(), "a preview follows");
        assert_eq!(e.single_repo(), None, "the form cannot read an address");
        // A search pasted there is read by the form.
        let _ = dialog.update(Message::Query("is:open label:bug".into()), &demo());
        assert_eq!(field_values(editor(&dialog).query(), Field::Labels), ["bug"]);
    }

    #[test]
    fn the_preview_waits_for_the_typing_to_stop() {
        let mut dialog = new_dialog();
        let editor = editor_mut(&mut dialog);
        let start = Instant::now();
        assert_eq!(editor.due(start + DEBOUNCE), None, "nothing changed");
        editor.searches[0] = "is:open".into();
        editor.touch(start);
        assert_eq!(editor.due(start + DEBOUNCE / 2), None, "still typing");
        editor.touch(start + DEBOUNCE / 2);
        assert_eq!(editor.due(start + DEBOUNCE), None, "the wait starts again with every change");
        assert_eq!(editor.due(start + DEBOUNCE / 2 + DEBOUNCE), Some(vec!["is:open".to_string()]));
        assert_eq!(editor.due(start + 3 * DEBOUNCE), None, "asked once");
        // Without a search the old preview goes and nothing is asked.
        editor.preview = Some(Ok(FilterPreview::default()));
        editor.searches[0] = " ".into();
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
        let _ = dialog.update(Message::Name(String::new()), &demo());
        let _ = dialog.update(Message::Query(String::new()), &demo());
        assert_eq!(editor(&dialog).name_problem(), None, "no error before the first try");
        let (_, outcome) = dialog.update(Message::Save, &demo());
        assert_eq!(outcome, Outcome::Stay);
        let editor_now = editor(&dialog);
        assert!(!editor_now.saving);
        assert_eq!(editor_now.name_problem(), Some(tr("Enter a name.")));
        assert!(editor_now.query_problem().is_some());
        let _ = dialog.update(Message::Name("Bugs".into()), &demo());
        let _ = dialog.update(Message::Save, &demo());
        assert!(editor(&dialog).expert && !editor(&dialog).saving, "GitHub search opens to show the missing search");
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
        assert_eq!((editor.current_name().as_str(), editor.color.as_deref()), ("Torro bugs", Some("#E8A33D")));
        assert_eq!(editor.queries(), ["is:issue label:bug", "is:pr"]);
        assert!(editor.changed.is_some(), "its preview follows");
        assert!(editor.expert, "two searches: GitHub search is open");
        assert!(!editor.back, "no way back to a choice");
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
    fn back_leads_to_the_choice_and_escape_closes_the_list_of_repositories_first() {
        let mut dialog = new_dialog();
        assert_eq!(dialog.update(Message::Back, &demo()).1, Outcome::Back);
        let _ = dialog.update(Message::TogglePicker, &demo());
        assert!(editor(&dialog).picker.is_some());
        assert_eq!(dialog.update(Message::Escape, &demo()).1, Outcome::Stay);
        assert!(editor(&dialog).picker.is_none(), "Escape closed the list");
        assert_eq!(dialog.update(Message::Escape, &demo()).1, Outcome::Close, "and then the dialog");
        let mut edit = Dialog::edit(&list(1, &["label:bug"]), GITHUB_ACCOUNT.into(), String::new()).expect("dialog");
        assert_eq!(edit.update(Message::Back, &demo()).1, Outcome::Stay, "editing has no choice to go back to");
    }

    #[test]
    fn a_new_list_preselects_the_repository_chosen_last() {
        // Chosen last (in another case, for another account – it is the same for all).
        let mut dialog = Dialog::new_list(start(Some("MAHYPE/torro-design"))).expect("dialog");
        assert_eq!(editor(&dialog).repo_label(), (tr("Loading repositories…").into(), true, false));
        repos_arrive(&mut dialog);
        let e = editor(&dialog);
        assert_eq!(e.query(), format!("{NEW_SEARCH} repo:{DESIGN}"));
        assert!(e.changed.is_some(), "its preview follows");
        assert_eq!(dialog.repo_to_remember().as_deref(), Some(DESIGN), "as GitHub writes it");
        // Not reached by the account, or none yet: the first repository.
        for last in [Some("someone/else"), None] {
            let mut dialog = Dialog::new_list(start(last)).expect("dialog");
            repos_arrive(&mut dialog);
            assert_eq!(dialog.repo_to_remember().as_deref(), Some(TORRO), "{last:?}");
        }
        // The list of repositories stays open while they arrive; Enter takes the first one found.
        let mut dialog = new_dialog();
        let _ = dialog.update(Message::TogglePicker, &demo());
        repos_arrive(&mut dialog);
        assert!(editor(&dialog).picker.is_some() && dialog.repo_to_remember().is_some());
        let _ = dialog.update(Message::FindRepo("NOTES".into()), &demo());
        assert_eq!(editor(&dialog).found().len(), 1);
        let _ = dialog.update(Message::PickFound, &demo());
        assert_eq!(
            (dialog.repo_to_remember().as_deref(), editor(&dialog).picker.is_none()),
            (Some("mahype/notes"), true)
        );
        // Repositories that could not be read: Retry asks again.
        let mut dialog = new_dialog();
        let _ = dialog.update(Message::Repos(GITHUB_ACCOUNT.into(), Err("offline".into())), &demo());
        assert_eq!(editor(&dialog).repos, Load::Failed("offline".into()));
        assert_eq!(dialog.repo_to_remember(), None);
        let (task, _) = dialog.update(Message::RetryRepos, &demo());
        assert!(task.units() > 0 && editor(&dialog).repos == Load::Loading);
        // Editing never changes the repository by itself.
        let mut edit =
            Dialog::edit(&list(1, &["is:issue repo:a/b"]), GITHUB_ACCOUNT.into(), String::new()).expect("dialog");
        repos_arrive(&mut edit);
        assert_eq!(edit.repo_to_remember().as_deref(), Some("a/b"));
    }

    #[test]
    fn chips_and_segments_write_the_search_and_read_it_back() {
        let mut dialog = new_on(TORRO);
        let base = format!("{NEW_SEARCH} repo:{TORRO}");
        assert_eq!(editor(&dialog).query(), base);
        for message in [
            Message::Label("bug".into()),
            Message::Label("good first issue".into()),
            Message::Without("ui".into()),
            Message::Pick(Field::Assignee, Pick::Nothing),
            Message::Pick(Field::Milestone, Pick::Value("v0.5".into())),
            Message::Pick(Field::Author, Pick::Me),
            Message::Kind(Kind::Both),
            Message::Sort(Sort::Updated),
        ] {
            let _ = dialog.update(message, &demo());
        }
        let query = editor(&dialog).query().to_string();
        assert_eq!(
            query,
            format!(
                "is:open repo:{TORRO} label:bug label:\"good first issue\" -label:ui no:assignee \
                 milestone:v0.5 author:@me sort:updated-desc"
            )
        );
        assert_eq!(field_values(&query, Field::Labels), ["bug", "good first issue"]);
        assert_eq!(
            (pick_of(&query, Field::Assignee), pick_of(&query, Field::Milestone), pick_of(&query, Field::Author)),
            (Pick::Nothing, Pick::Value("v0.5".into()), Pick::Me)
        );
        assert!(has_more(&query), "author, sort and a label without open More filters");
        // A label chosen on the other side moves; a chosen one clicked again goes.
        let _ = dialog.update(Message::Label("ui".into()), &demo());
        let _ = dialog.update(Message::Label("BUG".into()), &demo());
        let query = editor(&dialog).query().to_string();
        assert_eq!(
            (field_values(&query, Field::Labels), field_values(&query, Field::WithoutLabels)),
            (vec!["good first issue".to_string(), "ui".to_string()], Vec::new())
        );
        // Anyone, any milestone: the words go.
        let _ = dialog.update(Message::Pick(Field::Assignee, Pick::Any), &demo());
        let _ = dialog.update(Message::Pick(Field::Milestone, Pick::Any), &demo());
        let query = editor(&dialog).query().to_string();
        assert!(!query.contains("no:assignee") && !query.contains("milestone:"), "{query}");
        // A person of the repository.
        let _ = dialog.update(Message::Pick(Field::Assignee, Pick::Value("lena-k".into())), &demo());
        assert!(editor(&dialog).query().contains("assignee:lena-k"));
    }

    #[test]
    fn picks_keep_unknown_words_and_go_round_without_change() {
        let query = "is:open no:assignee -assignee:x no:milestone involves:@me";
        assert_eq!((pick_of(query, Field::Assignee), pick_of(query, Field::Milestone)), (Pick::Nothing, Pick::Nothing));
        assert_eq!(
            set_pick(query, Field::Assignee, &Pick::Me),
            "is:open assignee:@me -assignee:x no:milestone involves:@me"
        );
        assert_eq!(
            set_pick(query, Field::Milestone, &Pick::Value("v1 beta".into())),
            "is:open no:assignee -assignee:x milestone:\"v1 beta\" involves:@me"
        );
        assert_eq!(set_pick("author:anna", Field::Author, &Pick::Nothing), "", "authors have no no: word");
        assert_eq!(pick_of("milestone:@me", Field::Milestone), Pick::Value("@me".into()), "no Me for milestones");
        assert_eq!(toggle_label("label:bug,ui is:open", Field::Labels, "bug"), "label:bug,ui is:open label:bug");
        for query in ["", "no:assignee assignee:a", "author:@me review-requested:bob mentions:@me milestone:\"v1\""] {
            let mut out = query.to_string();
            for field in [Field::Assignee, Field::Author, Field::ReviewRequested, Field::Mentions, Field::Milestone] {
                out = set_pick(&out, field, &pick_of(&out, field));
            }
            assert_eq!(out, query);
        }
    }

    #[test]
    fn another_repository_takes_out_labels_and_milestone_it_does_not_have() {
        let mut dialog = new_on(TORRO);
        for message in [
            Message::Label("bug".into()),
            Message::Label("enhancement".into()),
            Message::Without("ui".into()),
            Message::Pick(Field::Milestone, Pick::Value("v0.4".into())),
        ] {
            let _ = dialog.update(message, &demo());
        }
        let _ = dialog.update(Message::PickRepo(DESIGN.into()), &demo());
        assert!(editor(&dialog).query().contains("label:bug"), "kept until the labels of the new one are known");
        assert_eq!(editor(&dialog).details(), Some(&Load::Loading), "asked for");
        described(&mut dialog, DESIGN);
        let query = editor(&dialog).query().to_string();
        assert_eq!(query, format!("{NEW_SEARCH} repo:{DESIGN} label:enhancement"));
        // Back to a repository whose labels are known: at once.
        let _ = dialog.update(Message::Label("tokens".into()), &demo());
        let _ = dialog.update(Message::PickRepo(TORRO.into()), &demo());
        assert_eq!(editor(&dialog).query(), format!("{NEW_SEARCH} repo:{TORRO} label:enhancement"));
        // Labels that could not be read: Retry asks again.
        let message =
            Message::Described { account: GITHUB_ACCOUNT.into(), repo: "mahype/notes".into(), result: Err("x".into()) };
        let _ = dialog.update(Message::PickRepo("mahype/notes".into()), &demo());
        let _ = dialog.update(message, &demo());
        assert_eq!(editor(&dialog).details(), Some(&Load::Failed("x".into())));
        let (task, _) = dialog.update(Message::RetryDetails, &demo());
        assert!(task.units() > 0 && editor(&dialog).details() == Some(&Load::Loading));
    }

    #[test]
    fn the_name_is_suggested_from_the_filters_until_one_is_typed() {
        let mut dialog = new_on(TORRO);
        let name = |dialog: &Dialog| editor(dialog).current_name();
        assert_eq!(name(&dialog), format!("{} · TorroWorkplace", tr("Open issues")));
        let _ = dialog.update(Message::Kind(Kind::PullRequests), &demo());
        let _ = dialog.update(Message::State(State::Any), &demo());
        assert_eq!(name(&dialog), format!("{} · TorroWorkplace", tr("Pull requests")));
        let _ = dialog.update(Message::Pick(Field::Assignee, Pick::Me), &demo());
        assert_eq!(name(&dialog), format!("{} · TorroWorkplace", tr("Assigned to me")));
        let _ = dialog.update(Message::Label("bug".into()), &demo());
        assert_eq!(name(&dialog), "Bug · TorroWorkplace");
        let _ = dialog.update(Message::Name("Mine".into()), &demo());
        let _ = dialog.update(Message::Label("ui".into()), &demo());
        assert_eq!(name(&dialog), "Mine", "typed: no more suggestions");
        // What is saved is the suggestion when nothing was typed.
        let mut dialog = new_on(TORRO);
        let _ = dialog.update(Message::Save, &demo());
        assert!(editor(&dialog).saving && editor(&dialog).name_problem().is_none());
    }

    #[test]
    fn editing_several_repositories_opens_the_search() {
        let several =
            Dialog::edit(&list(1, &["is:issue repo:a/b repo:c/d label:bug"]), GITHUB_ACCOUNT.into(), String::new())
                .expect("dialog");
        let e = editor(&several);
        assert!(e.expert);
        assert_eq!(e.repo_label(), (tr("Several repositories").into(), false, true));
        assert_eq!(several.repo_to_remember(), None, "no repository to remember");
        let two =
            Dialog::edit(&list(1, &["repo:a/b", "repo:c/d"]), GITHUB_ACCOUNT.into(), String::new()).expect("dialog");
        assert_eq!(editor(&two).repo_label().0, tr("Several repositories"));
        let one = Dialog::edit(&list(1, &["is:issue repo:a/b label:bug"]), GITHUB_ACCOUNT.into(), String::new())
            .expect("dialog");
        assert!(!editor(&one).expert && !editor(&one).more);
        assert_eq!(editor(&one).repo_label(), ("a/b".into(), false, false));
        let mut none =
            Dialog::edit(&list(1, &["is:open assignee:@me"]), GITHUB_ACCOUNT.into(), String::new()).expect("dialog");
        repos_arrive(&mut none);
        assert_eq!(editor(&none).repo_label(), (tr("All repositories").into(), true, false));
        let more = Dialog::edit(&list(1, &["repo:a/b sort:created-asc"]), GITHUB_ACCOUNT.into(), String::new())
            .expect("dialog");
        assert!(editor(&more).more, "More filters is open when something of it is set");
        assert_eq!(Sort::choices(&Sort::Oldest).last(), Some(&Sort::Oldest), "and the order shows as chosen");
        // Picking a repository there makes it the only one of the search.
        let mut several = several;
        let _ = several.update(Message::PickRepo(TORRO.into()), &demo());
        assert_eq!(editor(&several).query(), format!("is:issue repo:{TORRO} label:bug"));
    }

    #[test]
    fn another_account_asks_for_its_repositories() {
        let other = "github:work";
        let start = Start { accounts: vec![account(GITHUB_ACCOUNT), account(other)], ..start(None) };
        let mut dialog = Dialog::new_list(start).expect("dialog");
        repos_arrive(&mut dialog);
        described(&mut dialog, TORRO);
        let (task, _) = dialog.update(Message::Account(other.into()), &demo());
        let e = editor(&dialog);
        assert!(task.units() > 0, "its repositories are asked for");
        assert_eq!((e.account.as_str(), &e.repos), (other, &Load::Loading));
        assert_eq!((e.details.len(), e.details()), (1, Some(&Load::Loading)), "asked anew for this account");
        // A late answer for the first account changes nothing.
        repos_arrive(&mut dialog);
        assert_eq!(editor(&dialog).repos, Load::Loading);
        let reached = vec![GitHubRepo { repo: "work/site".into(), private: true }];
        let _ = dialog.update(Message::Repos(other.into(), Ok(reached)), &demo());
        assert_eq!(dialog.repo_to_remember().as_deref(), Some("work/site"), "the old one is out of its reach");
        let _ = dialog.update(Message::Account("github:nobody".into()), &demo());
        assert_eq!(editor(&dialog).account, other, "only offered accounts");
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
        let mut new = new_on(TORRO);
        let _ = new.update(Message::ToggleMore, &demo());
        let _ = new.update(Message::ToggleSearch, &demo());
        let _ = new.update(Message::TogglePicker, &demo());
        for size in [Size::new(1600.0, 1000.0), Size::new(800.0, 600.0), Size::new(480.0, 900.0)] {
            for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
                let _ = new.view(class, size);
            }
        }
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
