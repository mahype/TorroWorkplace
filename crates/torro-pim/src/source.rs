//! Where the apps get calendars and tasks from.
//!
//! The apps only know [`PimSource`]. In production that is the core over the socket
//! (`torro_ipc::pim::PimClient`); for building and trying the UIs it is
//! [`crate::demo::DemoSource`] with sample data in memory.
//!
//! Also here: how a list in "Manual order" is sorted and renumbered ([`manual_order`],
//! [`move_plan`], task P.3a), shared by the core, the demo data and the apps.
//!
//! GitHub filter lists are created, changed, deleted and tried out here as well
//! ([`FilterDefinition`], [`FilterPreview`]; the dialog of task G.4).

use std::cmp::Ordering;
use std::future::Future;

use chrono::{DateTime, Utc};

use crate::{Collection, Event, PimAccount, ProviderError, Todo};

pub trait PimSource: Send + Sync {
    /// Every account behind the data, in the order they were added; empty when none is set up.
    /// A sign-in that runs is listed too, with an empty `id` and [`crate::AccountState::SigningIn`].
    /// Apps that show a single state use [`crate::account::combined_state_for`] with the kind of
    /// collection they show; collections name their account in their id ([`crate::account`]).
    fn accounts(&self) -> impl Future<Output = Result<Vec<PimAccount>, ProviderError>> + Send;

    /// Starts Login Flow v2 for the Nextcloud at `server` (as the user typed it) and opens
    /// the browser; returns the login URL to show as a fallback link. The account is added to
    /// the others; signing in to one that exists again replaces its sign-in.
    fn connect_nextcloud(&self, server: String) -> impl Future<Output = Result<String, ProviderError>> + Send;

    /// Fetch everything from the servers of all accounts now. One account failing does not keep
    /// the others from syncing; the error is the first failure.
    fn sync(&self) -> impl Future<Output = Result<(), ProviderError>> + Send;

    fn collections(&self) -> impl Future<Output = Result<Vec<Collection>, ProviderError>> + Send;

    /// Events overlapping `[start, end)`, recurring ones expanded into occurrences.
    fn events(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<Event>, ProviderError>> + Send;

    /// All tasks, completed ones included.
    fn todos(&self) -> impl Future<Output = Result<Vec<Todo>, ProviderError>> + Send;

    /// Creates (`href: None`) or updates an event; returns it as stored, with `href` and
    /// `etag` set. An outdated `etag` fails with [`ProviderError::Conflict`].
    fn save_event(&self, event: Event) -> impl Future<Output = Result<Event, ProviderError>> + Send;

    fn delete_event(&self, event: Event) -> impl Future<Output = Result<(), ProviderError>> + Send;

    /// Creates (`href: None`) or updates a task; returns it as stored.
    fn save_todo(&self, todo: Todo) -> impl Future<Output = Result<Todo, ProviderError>> + Send;

    fn delete_todo(&self, todo: Todo) -> impl Future<Output = Result<(), ProviderError>> + Send;

    /// Puts `todo` right before `before` among the tasks it is ordered with, or at the end
    /// without `before`: the move of a list in "Manual order" (`tasks.move_todo`, task P.3a).
    /// Both are tasks as [`PimSource::todos`] returned them (`href` and `etag` set). Returns the
    /// tasks whose `sort_order` changed, as stored with their new `etag`; empty when `todo` is
    /// there already. Replace them in the list the app holds and sort with [`manual_order`].
    ///
    /// [`ProviderError::Conflict`] when `todo` changed meanwhile (reload and move again);
    /// [`ProviderError::Other`] when `before` is in another list, has another parent, or is
    /// done while `todo` is open (or the other way round). When one of several writes fails,
    /// the ones before it stay and `todo`, written last, has not moved: reload the tasks.
    fn move_todo(
        &self,
        todo: Todo,
        before: Option<Todo>,
    ) -> impl Future<Output = Result<Vec<Todo>, ProviderError>> + Send;

    /// Takes the pinned issue or pull request `item` out of the GitHub filter list `collection`
    /// (`tasks.unpin_issue`, task G.2; "Unpin" in the tasks app, task G.3). `collection` is the
    /// list's [`Collection::id`] (`github:<login>/<list-id>`), `item` the link id as
    /// `source.pinned` lists it (`github:owner/repo#12`; the core also takes the item's address on
    /// github.com). The item stays in the list while one of its searches finds it; nothing changes
    /// on GitHub. Reload the tasks afterwards.
    ///
    /// [`ProviderError::Other`] when the list does not pin the item or `collection` is no filter
    /// list (the core's `not-found`).
    fn unpin_issue(&self, collection: String, item: String) -> impl Future<Output = Result<(), ProviderError>> + Send;

    /// Adds the GitHub issue or pull request `url` to the task list `collection`
    /// (`tasks.add_link`, task G.6): pinned in a GitHub filter list, a *link* (a task with the
    /// item's address, its link id and GitHub's title) in a writable list. `url` is what the person
    /// pasted: the item's address on github.com, `owner/repo#12` or `github:owner/repo#12`.
    ///
    /// Returns the task as the list holds it now, with the GitHub details in `link.github`; when
    /// the list holds the item already, that task. Reload the tasks afterwards (pinning fetches
    /// the filter list again).
    ///
    /// [`ProviderError::Other`] with the core's message when `url` names no issue or pull request,
    /// GitHub does not have it, no GitHub account is connected, or the list is read-only or unknown;
    /// [`ProviderError::Auth`] / [`ProviderError::Offline`] when GitHub refuses the token or cannot
    /// be reached.
    fn add_link(&self, collection: String, url: String) -> impl Future<Output = Result<Todo, ProviderError>> + Send;

    /// Creates a GitHub filter list of the GitHub account `account` (`github:<login>`, as
    /// [`PimSource::accounts`] lists it) from `definition` (`tasks.create_filter_list`, dialog of
    /// task G.4). Returns the new list as [`PimSource::collections`] will list it: id
    /// `github:<login>/<list-id>`, not writable, the definition in `source` (searches as the core
    /// stores them: addresses turned into searches); a failed first fetch is in `source.problem`
    /// and the list is kept. Reload collections and tasks afterwards for its items.
    ///
    /// [`ProviderError::Other`] with the core's message for an account that is no connected
    /// GitHub account, an empty name, a colour that is not `#RRGGBB`, no search or more than ten,
    /// an address that is no issue list, and a search a client may not run (`forbidden`).
    fn create_filter_list(
        &self,
        account: String,
        definition: FilterDefinition,
    ) -> impl Future<Output = Result<Collection, ProviderError>> + Send;

    /// Replaces name, colour and searches of the filter list `collection` with `definition` and
    /// fetches it again; pinned items stay (`tasks.update_filter_list`). `revision` is the list's
    /// `source.revision` as last read. Returns the list as stored, with its new revision.
    ///
    /// [`ProviderError::Conflict`] (its `uid` is `collection`) when the list changed meanwhile:
    /// nothing changed, read the list again. [`ProviderError::Other`] as for
    /// [`PimSource::create_filter_list`], and for a collection that is no filter list.
    fn update_filter_list(
        &self,
        collection: String,
        revision: u64,
        definition: FilterDefinition,
    ) -> impl Future<Output = Result<Collection, ProviderError>> + Send;

    /// Deletes the filter list `collection` with its cached items from this device
    /// (`tasks.delete_filter_list`). Nothing changes on GitHub. Reload collections and tasks
    /// afterwards. [`ProviderError::Other`] for a collection that is no filter list.
    fn delete_filter_list(&self, collection: String) -> impl Future<Output = Result<(), ProviderError>> + Send;

    /// Runs `queries` (searches or github.com addresses of issue lists, as for
    /// [`FilterDefinition::queries`]) for the GitHub account `account` without saving anything
    /// (`tasks.preview_filter`): one call to GitHub per search, so call it when the person stops
    /// typing, not on every key.
    ///
    /// [`ProviderError::Auth`] / [`ProviderError::Offline`] when GitHub refuses the token or cannot
    /// be reached; [`ProviderError::Other`] with the core's message for an address that is no
    /// issue list, a search GitHub cannot run, an account without a token on this device
    /// (`not-connected`) and a search a client may not run (`forbidden`).
    fn preview_filter(
        &self,
        account: String,
        queries: Vec<String>,
    ) -> impl Future<Output = Result<FilterPreview, ProviderError>> + Send;
}

/// What a GitHub filter list is made of, as the dialog of task G.4 sends it to
/// [`PimSource::create_filter_list`] and [`PimSource::update_filter_list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterDefinition {
    /// The name of the list; must not be empty.
    pub name: String,
    /// The colour as `#RRGGBB`; `None` for the app's default.
    pub color: Option<String>,
    /// One to ten GitHub searches (`is:open is:issue repo:owner/name label:bug`) or addresses of
    /// issue lists on github.com, which the core turns into searches. The list holds the matches
    /// of any of them.
    pub queries: Vec<String>,
}

/// What searches find without being saved ([`PimSource::preview_filter`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterPreview {
    /// How many issues and pull requests match: GitHub's count, added up over the searches (an
    /// item two searches find counts twice).
    pub matches: u64,
    /// A search found more than 1,000 matches; a list of it would hold only the first 1,000.
    pub truncated: bool,
    /// The first matches (at most 20) as tasks of no list (`collection` empty), with their GitHub
    /// details in `link`.
    pub todos: Vec<Todo>,
}

/// Room left behind the last number when a task goes to the end of a list or a task without
/// number gets one, so later moves find a gap and rarely renumber.
pub const GAP: i64 = 1024;

/// How the tasks of a list in "Manual order" are sorted, as Nextcloud Tasks sorts a list it
/// orders by hand: smaller `sort_order` (`X-APPLE-SORT-ORDER`) first.
///
/// Nextcloud Tasks gives a task without `X-APPLE-SORT-ORDER` the seconds from 2001-01-01 to its
/// `CREATED` (`src/models/task.js`, `getSortOrder`), which the model does not read. Here such
/// tasks come after every task with a number, which is where Nextcloud Tasks puts the tasks it
/// creates (it writes no number for them). Ties go by `uid`, so every app shows the same order.
pub fn manual_order(a: &Todo, b: &Todo) -> Ordering {
    fn key(t: &Todo) -> (bool, Option<i64>, &str) {
        (t.sort_order.is_none(), t.sort_order, t.uid.as_str())
    }
    key(a).cmp(&key(b))
}

/// `a` and `b` are ordered among each other: the same list, the same parent, both open or both
/// done (Nextcloud Tasks orders subtasks under their parent and done tasks in their own section).
pub fn same_level(a: &Todo, b: &Todo) -> bool {
    a.collection == b.collection && a.parent == b.parent && a.completed == b.completed
}

/// The writes that put `todo` right before `before` (at the end without) among the tasks of
/// `todos` on its level ([`same_level`]), sorted by [`manual_order`]. Tasks are told apart by
/// `href`. Shared by the core and [`crate::demo::DemoSource`], so both number the same way.
///
/// Returns the tasks to write, each with its new `sort_order`, in the order to write them: `todo`
/// comes last, so a failed write leaves it where it was. Empty when `todo` is right before
/// `before` (or last) already, or `before` is `todo` itself. `None` when `before` is not on the
/// level of `todo`.
///
/// Like Nextcloud Tasks (`src/components/TaskDragContainer.vue`, `adjustSortOrder`) only tasks
/// whose number has to change are written, and new numbers are 0 or above:
///
/// - `todo` gets a number between its new neighbours: the middle of the gap when there is one,
///   else the one after its predecessor; at the end the last number plus [`GAP`];
/// - a following task that would no longer come after its predecessor gets its number plus one,
///   which may carry on down the list (renumbering only as far as needed);
/// - tasks without a number in front of the new place get one each ([`GAP`] apart), so they stay
///   in front; those behind it keep none and stay at the end.
pub fn move_plan(todos: &[Todo], todo: &Todo, before: Option<&Todo>) -> Option<Vec<Todo>> {
    let mut level: Vec<&Todo> = todos.iter().filter(|t| same_level(t, todo) && t.href != todo.href).collect();
    level.sort_by(|a, b| manual_order(a, b));
    let at = match before {
        None => level.len(),
        Some(before) if before.href == todo.href => return Some(Vec::new()),
        Some(before) => level.iter().position(|t| t.href == before.href)?,
    };
    // Where it stands now; right before the task at `at` (or last) means nothing to do.
    let now = level.iter().filter(|t| manual_order(t, todo) == Ordering::Less).count();
    if now == at {
        return Some(Vec::new());
    }
    level.insert(at, todo);
    let mut numbers: Vec<Option<i64>> = Vec::with_capacity(level.len());
    // Number and uid of the task in front, as it will be stored.
    let mut last: Option<(i64, &str)> = None;
    for (i, task) in level.iter().enumerate() {
        let number = if i == at {
            Some(place_between(last.map(|(n, _)| n), level.get(i + 1).and_then(|t| t.sort_order)))
        } else {
            match (task.sort_order, last) {
                (Some(n), Some((front, uid))) if (n, task.uid.as_str()) <= (front, uid) => {
                    Some(front.saturating_add(1))
                }
                (Some(n), _) => Some(n),
                (None, _) if i < at => Some(last.map_or(0, |(n, _)| n.saturating_add(GAP))),
                // Behind the new place: still without, still at the end.
                (None, _) => None,
            }
        };
        if let Some(n) = number {
            last = Some((n, task.uid.as_str()));
        }
        numbers.push(number);
    }
    let changed = |i: &usize| numbers[*i] != level[*i].sort_order;
    let write = |i: usize| Todo { sort_order: numbers[i], ..level[i].clone() };
    // Numbered tasks in front first, then the renumbered ones from the end up, `todo` last.
    let mut plan: Vec<Todo> = (0..at).filter(changed).map(write).collect();
    plan.extend((at + 1..level.len()).rev().filter(changed).map(write));
    plan.extend(Some(at).filter(changed).map(write));
    Some(plan)
}

/// The number for a task placed behind one numbered `front` (`None`: at the top) and in front of
/// one numbered `behind` (`None`: at the end, or in front of tasks without number). Never below 0.
fn place_between(front: Option<i64>, behind: Option<i64>) -> i64 {
    // At the top the gap starts below 0, so the first number there is 0 at the lowest.
    let low = front.unwrap_or(-1);
    match behind {
        Some(high) if high.saturating_sub(low) >= 2 => low + (high - low) / 2,
        Some(_) => low.saturating_add(1),
        None => front.map_or(0, |n| n.saturating_add(GAP)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A task `uid` in the list `list` with `sort_order`, stored at `<list><uid>.ics`.
    fn task(list: &str, uid: &str, sort_order: Option<i64>) -> Todo {
        Todo {
            uid: uid.into(),
            collection: list.into(),
            href: Some(format!("{list}{uid}.ics")),
            etag: Some("\"1\"".into()),
            summary: uid.into(),
            due: None,
            priority: None,
            completed: false,
            parent: None,
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order,
            link: None,
        }
    }

    /// Tasks named by `uids` in the list `/l/` with these numbers.
    fn list(tasks: &[(&str, Option<i64>)]) -> Vec<Todo> {
        tasks.iter().map(|(uid, n)| task("/l/", uid, *n)).collect()
    }

    fn by_uid<'a>(todos: &'a [Todo], uid: &str) -> &'a Todo {
        todos.iter().find(|t| t.uid == uid).expect("a task with this uid")
    }

    /// Moves `uid` before `before` and returns the writes as `(uid, number)` and the order the
    /// list shows afterwards.
    fn moved(todos: &[Todo], uid: &str, before: Option<&str>) -> (Vec<(String, Option<i64>)>, Vec<String>) {
        let before = before.map(|b| by_uid(todos, b));
        let plan = move_plan(todos, by_uid(todos, uid), before).expect("same level");
        let writes = plan.iter().map(|t| (t.uid.clone(), t.sort_order)).collect();
        let mut after: Vec<Todo> = todos.to_vec();
        for written in plan {
            if let Some(slot) = after.iter_mut().find(|t| t.href == written.href) {
                *slot = written;
            }
        }
        after.sort_by(manual_order);
        (writes, after.into_iter().map(|t| t.uid).collect())
    }

    fn w(uid: &str, n: i64) -> (String, Option<i64>) {
        (uid.to_string(), Some(n))
    }

    fn abc() -> Vec<Todo> {
        list(&[("a", Some(100)), ("b", Some(200)), ("c", Some(300))])
    }

    #[test]
    fn to_the_top_uses_the_room_above_the_first() {
        assert_eq!(moved(&abc(), "c", Some("a")), (vec![w("c", 49)], vec!["c".into(), "a".into(), "b".into()]));
    }

    #[test]
    fn into_the_middle_takes_the_middle_of_the_gap() {
        assert_eq!(moved(&abc(), "a", Some("c")), (vec![w("a", 250)], vec!["b".into(), "a".into(), "c".into()]));
    }

    #[test]
    fn to_the_end_leaves_a_gap_behind_the_last() {
        assert_eq!(moved(&abc(), "a", None), (vec![w("a", 300 + GAP)], vec!["b".into(), "c".into(), "a".into()]));
    }

    #[test]
    fn without_a_gap_the_tasks_behind_are_renumbered_only_as_far_as_needed() {
        let todos = list(&[("p", Some(1)), ("o", Some(2)), ("n", Some(3)), ("k", Some(10)), ("z", Some(20))]);
        // z goes before o (2): it takes 2, o becomes 3, n 4; k (10) comes after them already.
        let (writes, order) = moved(&todos, "z", Some("o"));
        assert_eq!(order, ["p", "z", "o", "n", "k"]);
        assert_eq!(writes, vec![w("n", 4), w("o", 3), w("z", 2)], "from the end up, the moved task last");
    }

    #[test]
    fn at_the_top_numbers_stay_at_zero_or_above() {
        let todos = list(&[("a", Some(0)), ("b", Some(1)), ("c", Some(5))]);
        let (writes, order) = moved(&todos, "c", Some("a"));
        assert_eq!(order, ["c", "a", "b"]);
        // a becomes 1 like b, and stays in front of it by uid.
        assert_eq!(writes, vec![w("a", 1), w("c", 0)]);
    }

    #[test]
    fn ties_are_kept_in_uid_order_and_broken_where_needed() {
        let todos = list(&[("a", Some(3)), ("b", Some(3)), ("c", Some(3))]);
        let (writes, order) = moved(&todos, "a", Some("c"));
        assert_eq!(order, ["b", "a", "c"]);
        assert_eq!(writes, vec![w("c", 5), w("a", 4)]);
    }

    #[test]
    fn tasks_without_a_number_come_last_and_get_one_only_when_something_goes_behind_them() {
        let todos = list(&[("a", None), ("b", Some(5)), ("c", None)]);
        let mut sorted = todos.clone();
        sorted.sort_by(manual_order);
        assert_eq!(sorted.iter().map(|t| t.uid.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
        // b to the end: a and c are numbered in front of it, in their order.
        let (writes, order) = moved(&todos, "b", None);
        assert_eq!(order, ["a", "c", "b"]);
        assert_eq!(writes, vec![w("a", 0), w("c", GAP), w("b", 2 * GAP)]);
        // c before a: only c gets a number; a stays without and so behind it.
        assert_eq!(moved(&todos, "c", Some("a")), (vec![w("c", 5 + GAP)], vec!["b".into(), "c".into(), "a".into()]));
    }

    #[test]
    fn a_list_without_any_numbers_starts_at_zero() {
        let todos = list(&[("a", None), ("b", None), ("c", None)]);
        assert_eq!(moved(&todos, "c", Some("a")), (vec![w("c", 0)], vec!["c".into(), "a".into(), "b".into()]));
    }

    #[test]
    fn a_task_in_its_place_already_writes_nothing() {
        let todos = list(&[("a", Some(1)), ("b", Some(2)), ("c", None)]);
        let plan = |uid: &str, before: Option<&str>| {
            move_plan(&todos, by_uid(&todos, uid), before.map(|b| by_uid(&todos, b))).expect("same level")
        };
        assert!(plan("a", Some("b")).is_empty());
        assert!(plan("c", None).is_empty(), "last already");
        assert!(plan("b", Some("b")).is_empty(), "before itself");
    }

    #[test]
    fn only_tasks_on_the_same_level_count() {
        let mut todos = list(&[("a", Some(1)), ("b", Some(2)), ("c", Some(3))]);
        let done = Todo { completed: true, ..task("/l/", "done", Some(0)) };
        let child = Todo { parent: Some("a".into()), ..task("/l/", "child", Some(0)) };
        let other = task("/other/", "x", Some(0));
        todos.extend([done.clone(), child.clone(), other.clone()]);
        let (writes, order) = moved(&todos, "c", Some("a"));
        assert_eq!(writes, vec![w("c", 0)]);
        let ours: Vec<&String> = order.iter().filter(|uid| ["a", "b", "c"].contains(&uid.as_str())).collect();
        assert_eq!(ours, ["c", "a", "b"]);
        for stranger in [&done, &child, &other] {
            assert_eq!(move_plan(&todos, by_uid(&todos, "c"), Some(stranger)), None, "{}", stranger.uid);
        }
    }
}
