//! The history of postponed tasks (issue #623, `docs/aufschieben.md`): which open task the core moved to a
//! later due day, when, and what priority and focus it had then – for `tasks.postpone_stats`.
//!
//! It stays on this computer (`postponements.json` in the core's state directory, 0600) and never goes into
//! the task: task lists are shared, and how a person puts things off is theirs alone. No title is stored;
//! the evaluation takes titles from the core's cache when it is read. Entries older than the days the
//! setting `postpone_retention` keeps ([`PostponeRetention`], 365 unless changed) are dropped whenever the
//! history is read or written, and at once when the setting is lowered ([`Postponements::tidy`]).

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc, Weekday};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_core::settings::PostponeRetention;
use torro_ipc::{PostponeCount, PostponeStats, PostponedTask};
use torro_pim::source::due_day;
use torro_pim::{Focus, Todo};

/// The file in the core's state directory.
const FILE: &str = "postponements.json";

/// One move of an open task to a later due day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Postponement {
    /// iCalendar UID of the task: it stays when the task moves to another list, its `href` does not.
    pub uid: String,
    /// The task list it was in.
    pub collection: String,
    /// The day it was due on.
    pub from: NaiveDate,
    /// The day it became due on.
    pub to: NaiveDate,
    /// When it was moved.
    pub at: DateTime<Utc>,
    /// Its iCalendar priority then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    /// Its focus then.
    #[serde(default)]
    pub focus: Focus,
}

/// What can go wrong with the history's file.
#[derive(Debug)]
pub enum PostponementsError {
    Io { path: PathBuf, err: io::Error },
    Parse { path: PathBuf, err: serde_json::Error },
}

impl std::fmt::Display for PostponementsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PostponementsError::Io { path, err } => write!(f, "cannot read or write {}: {err}", path.display()),
            PostponementsError::Parse { path, err } => {
                write!(f, "{} is not a history of postponed tasks: {err}", path.display())
            }
        }
    }
}

impl std::error::Error for PostponementsError {}

/// The file's content.
#[derive(Debug, Default, Serialize, Deserialize)]
struct HistoryFile {
    #[serde(default)]
    postponements: Vec<Postponement>,
}

/// The history, read from its file on first use and written after every change. Without a file (tests,
/// no home directory) it lives in memory only.
pub struct Postponements {
    file: Option<PathBuf>,
    /// `None` until read.
    entries: Mutex<Option<Vec<Postponement>>>,
}

impl Postponements {
    /// The user's history; tests keep theirs in memory.
    pub fn system() -> Postponements {
        if cfg!(test) {
            return Postponements::new(None);
        }
        Postponements::new(AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join(FILE)))
    }

    /// The history kept in `file`.
    pub fn new(file: Option<PathBuf>) -> Postponements {
        Postponements { file, entries: Mutex::new(None) }
    }

    /// Adds `found` and drops what is older than `kept` at `now`. Nothing is written for nothing found.
    pub async fn record(
        &self,
        found: Vec<Postponement>,
        now: DateTime<Utc>,
        kept: PostponeRetention,
    ) -> Result<(), PostponementsError> {
        if found.is_empty() {
            return Ok(());
        }
        let mut entries = self.entries.lock().await;
        let mut list = self.loaded(&mut entries).await?;
        list.extend(found);
        tidy(&mut list, now, kept);
        self.write(&list).await?;
        *entries = Some(list);
        Ok(())
    }

    /// Every postponement kept for `kept` at `now`, oldest first.
    pub async fn list(
        &self,
        now: DateTime<Utc>,
        kept: PostponeRetention,
    ) -> Result<Vec<Postponement>, PostponementsError> {
        let mut entries = self.entries.lock().await;
        let mut list = self.loaded(&mut entries).await?;
        tidy(&mut list, now, kept);
        *entries = Some(list.clone());
        Ok(list)
    }

    /// Drops what is older than `kept` at `now` and writes the file when something went: for a lowered
    /// `postpone_retention`, which takes effect at once and not only with the next postponement. Returns
    /// how many went.
    pub async fn tidy(&self, now: DateTime<Utc>, kept: PostponeRetention) -> Result<usize, PostponementsError> {
        let mut entries = self.entries.lock().await;
        let mut list = self.loaded(&mut entries).await?;
        let before = list.len();
        tidy(&mut list, now, kept);
        let removed = before - list.len();
        if removed > 0 {
            self.write(&list).await?;
        }
        *entries = Some(list);
        Ok(removed)
    }

    /// Deletes the postponements of the lists `lists` names, all of them for `None`; returns how many.
    pub async fn clear(&self, lists: Option<&HashSet<String>>) -> Result<usize, PostponementsError> {
        let mut entries = self.entries.lock().await;
        let mut list = self.loaded(&mut entries).await?;
        let before = list.len();
        list.retain(|p| lists.is_some_and(|lists| !lists.contains(&p.collection)));
        let removed = before - list.len();
        if removed > 0 {
            self.write(&list).await?;
        }
        *entries = Some(list);
        Ok(removed)
    }

    /// The entries, read from the file when they are not yet; a missing file is an empty history.
    async fn loaded(&self, entries: &mut Option<Vec<Postponement>>) -> Result<Vec<Postponement>, PostponementsError> {
        if let Some(list) = entries {
            return Ok(list.clone());
        }
        let Some(file) = self.file.clone() else {
            return Ok(Vec::new());
        };
        let read = tokio::task::spawn_blocking(move || match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice::<HistoryFile>(&bytes)
                .map(|content| content.postponements)
                .map_err(|err| PostponementsError::Parse { path: file, err }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(PostponementsError::Io { path: file, err }),
        })
        .await
        .map_err(|err| self.io(io::Error::other(err.to_string())))??;
        Ok(read)
    }

    /// Writes `list` to the file (0600, its directory for the owner only); nothing without a file.
    async fn write(&self, list: &[Postponement]) -> Result<(), PostponementsError> {
        let Some(file) = self.file.clone() else {
            return Ok(());
        };
        let content = HistoryFile { postponements: list.to_vec() };
        tokio::task::spawn_blocking(move || {
            let bytes = serde_json::to_vec_pretty(&content)
                .map_err(|err| PostponementsError::Parse { path: file.clone(), err })?;
            if let Some(dir) = file.parent() {
                crate::clients::create_private_dir(dir)
                    .map_err(|err| PostponementsError::Io { path: file.clone(), err })?;
            }
            crate::clients::write_private(&file, &bytes)
                .map_err(|err| PostponementsError::Io { path: file.clone(), err: io::Error::other(err.to_string()) })
        })
        .await
        .map_err(|err| self.io(io::Error::other(err.to_string())))?
    }

    fn io(&self, err: io::Error) -> PostponementsError {
        PostponementsError::Io { path: self.file.clone().unwrap_or_else(|| PathBuf::from(FILE)), err }
    }
}

/// Drops what is older than `kept` at `now`.
fn tidy(list: &mut Vec<Postponement>, now: DateTime<Utc>, kept: PostponeRetention) {
    let oldest = now - Duration::days(i64::from(kept.days()));
    list.retain(|p| p.at >= oldest);
}

/// The postponements among `written` (the tasks as a command stored them) against `before` (the same tasks
/// as cached before it), by `href`, or by UID when the save moved it to another list: an open task that was due on a day and is due on a later day now, days
/// taken in the zone `tz`. A new task, one without a due date before or after, one done, and one moved to the
/// same or an earlier day are none (`docs/aufschieben.md`, decision 2).
pub fn found<Z: TimeZone>(before: &[Todo], written: &[Todo], tz: &Z, now: DateTime<Utc>) -> Vec<Postponement> {
    let by_href: HashMap<&str, &Todo> = before.iter().filter_map(|t| t.href.as_deref().map(|href| (href, t))).collect();
    let by_uid: HashMap<&str, &Todo> = before.iter().map(|t| (t.uid.as_str(), t)).collect();
    written
        .iter()
        .filter(|after| !after.completed)
        .filter_map(|after| {
            let href = after.href.as_deref()?;
            let was = by_href.get(href).or_else(|| by_uid.get(after.uid.as_str()))?;
            let from = due_day(was, tz)?;
            let to = due_day(after, tz)?;
            (to > from && !was.completed).then(|| Postponement {
                uid: after.uid.clone(),
                collection: after.collection.clone(),
                from,
                to,
                at: now,
                priority: was.priority,
                focus: was.focus,
            })
        })
        .collect()
}

/// The evaluation of `entries` (`tasks.postpone_stats`): those of the lists `lists` names (all for `None`)
/// from the day `since` on, the tasks found in `cached` (by UID) with their `href`, title and state.
pub fn stats(
    entries: &[Postponement],
    since: Option<NaiveDate>,
    lists: Option<&HashSet<String>>,
    cached: &[Todo],
) -> PostponeStats {
    let counted: Vec<&Postponement> = entries
        .iter()
        .filter(|p| lists.is_none_or(|lists| lists.contains(&p.collection)))
        .filter(|p| since.is_none_or(|since| p.at.date_naive() >= since))
        .collect();
    let mut tasks: HashMap<&str, PostponedTask> = HashMap::new();
    for p in &counted {
        let task = tasks.entry(&p.uid).or_insert_with(|| PostponedTask {
            uid: p.uid.clone(),
            collection: p.collection.clone(),
            count: 0,
            last: p.at,
            href: None,
            title: None,
            done: false,
        });
        task.count += 1;
        if p.at >= task.last {
            task.last = p.at;
            task.collection = p.collection.clone();
        }
    }
    let mut tasks: Vec<PostponedTask> = tasks.into_values().collect();
    for task in &mut tasks {
        if let Some(todo) = cached.iter().find(|t| t.uid == task.uid) {
            task.href = todo.href.clone();
            task.title = Some(todo.summary.clone());
            task.done = todo.completed;
        }
    }
    tasks.sort_by(|a, b| b.count.cmp(&a.count).then(b.last.cmp(&a.last)).then(a.uid.cmp(&b.uid)));
    PostponeStats {
        enabled: true,
        since: counted.iter().map(|p| p.at.date_naive()).min(),
        total: counted.len(),
        tasks,
        by_priority: grouped(&counted, |p| priority_class(p.priority).into()),
        by_focus: grouped(&counted, |p| focus_key(p.focus).into()),
        by_list: grouped(&counted, |p| p.collection.clone()),
        by_weekday: grouped(&counted, |p| weekday_key(p.from.weekday()).into()),
    }
}

/// The counts of `counted` by `key`, most first, then by key.
fn grouped(counted: &[&Postponement], key: impl Fn(&Postponement) -> String) -> Vec<PostponeCount> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for p in counted {
        *counts.entry(key(p)).or_default() += 1;
    }
    let mut groups: Vec<PostponeCount> = counts.into_iter().map(|(key, count)| PostponeCount { key, count }).collect();
    groups.sort_by(|a, b| b.count.cmp(&a.count).then(a.key.cmp(&b.key)));
    groups
}

/// The tasks app's five priority classes (`torrotasks::list::Prio::of`): iCalendar 1 urgent, 2–4 high, 5
/// medium, 6–9 low, anything else none.
fn priority_class(priority: Option<u8>) -> &'static str {
    match priority {
        Some(1) => "urgent",
        Some(2..=4) => "high",
        Some(5) => "medium",
        Some(6..=9) => "low",
        _ => "none",
    }
}

/// The focus as the API spells it (`torro_pim::Focus`, snake case).
fn focus_key(focus: Focus) -> &'static str {
    match focus {
        Focus::Normal => "normal",
        Focus::Focus => "focus",
        Focus::Strong => "strong",
    }
}

fn weekday_key(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "mon",
        Weekday::Tue => "tue",
        Weekday::Wed => "wed",
        Weekday::Thu => "thu",
        Weekday::Fri => "fri",
        Weekday::Sat => "sat",
        Weekday::Sun => "sun",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_pim::EventTime;

    const YEAR: PostponeRetention = PostponeRetention::DEFAULT;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).expect("a day")
    }

    fn at(d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, d, 17, 0, 0).single().expect("an instant")
    }

    fn task(uid: &str, due: Option<NaiveDate>) -> Todo {
        Todo {
            uid: uid.into(),
            collection: "home".into(),
            href: Some(format!("/tasks/home/{uid}.ics")),
            etag: Some("1".into()),
            summary: format!("Task {uid}"),
            due: due.map(EventTime::Date),
            priority: Some(3),
            completed: false,
            parent: None,
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
            focus: Focus::Focus,
        }
    }

    fn moved(uid: &str, from: u32, to: u32) -> Postponement {
        Postponement {
            uid: uid.into(),
            collection: "home".into(),
            from: day(from),
            to: day(to),
            at: at(from),
            priority: Some(3),
            focus: Focus::Focus,
        }
    }

    #[test]
    fn only_a_later_day_of_an_open_task_with_a_due_date_counts() {
        let before = vec![
            task("later", Some(day(20))),
            task("earlier", Some(day(20))),
            task("same", Some(day(20))),
            task("dated", None),
            task("undated", Some(day(20))),
            task("done", Some(day(20))),
        ];
        let after = vec![
            task("later", Some(day(21))),
            task("earlier", Some(day(19))),
            task("same", Some(day(20))),
            task("dated", Some(day(22))),
            task("undated", None),
            Todo { completed: true, ..task("done", Some(day(23))) },
            task("new", Some(day(25))),
        ];
        let found = found(&before, &after, &Utc, at(20));
        assert_eq!(found, vec![Postponement { at: at(20), ..moved("later", 20, 21) }]);

        // Moved to another list and to a later day in one save: found by its UID, counted in the new list.
        let elsewhere = Todo {
            collection: "work".into(),
            href: Some("/tasks/work/later.ics".into()),
            ..task("later", Some(day(22)))
        };
        let moved_over = super::found(&before, &[elsewhere], &Utc, at(20));
        assert_eq!(moved_over.iter().map(|p| (p.collection.as_str(), p.to)).collect::<Vec<_>>(), [("work", day(22))]);
    }

    #[test]
    fn an_instant_counts_by_its_local_day_and_keeps_what_the_task_was() {
        let morning = Utc.with_ymd_and_hms(2026, 11, 20, 9, 0, 0).single().expect("an instant");
        let was = Todo { due: Some(EventTime::DateTime(morning)), priority: Some(1), ..task("t", None) };
        let later_that_day = Todo { due: Some(EventTime::DateTime(morning + Duration::hours(6))), ..was.clone() };
        assert!(found(std::slice::from_ref(&was), &[later_that_day], &Utc, at(20)).is_empty(), "same day");
        let next_day = Todo {
            due: Some(EventTime::DateTime(morning + Duration::days(1))),
            priority: Some(9),
            focus: Focus::Normal,
            ..was.clone()
        };
        let found = found(&[was], &[next_day], &Utc, at(20));
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].priority, found[0].focus), (Some(1), Focus::Focus), "as it was when moved");
    }

    #[test]
    fn the_evaluation_counts_by_task_priority_focus_list_and_weekday() {
        let mut entries = vec![moved("a", 16, 17), moved("a", 17, 18), moved("b", 18, 19)];
        entries.push(Postponement {
            collection: "work".into(),
            priority: None,
            focus: Focus::Strong,
            ..moved("c", 20, 23)
        });
        let cached = vec![task("a", Some(day(18))), Todo { completed: true, ..task("b", Some(day(19))) }];
        let stats = stats(&entries, None, None, &cached);
        assert!(stats.enabled);
        assert_eq!((stats.total, stats.since), (4, Some(day(16))));
        let tasks: Vec<_> = stats.tasks.iter().map(|t| (t.uid.as_str(), t.count, t.title.is_some(), t.done)).collect();
        assert_eq!(tasks, [("a", 2, true, false), ("c", 1, false, false), ("b", 1, true, true)]);
        assert_eq!(stats.tasks[0].href.as_deref(), Some("/tasks/home/a.ics"));
        let keys = |groups: &[PostponeCount]| groups.iter().map(|g| (g.key.clone(), g.count)).collect::<Vec<_>>();
        assert_eq!(keys(&stats.by_priority), [("high".into(), 3), ("none".into(), 1)]);
        assert_eq!(keys(&stats.by_focus), [("focus".into(), 3), ("strong".into(), 1)]);
        assert_eq!(keys(&stats.by_list), [("home".into(), 3), ("work".into(), 1)]);
        // 16 Nov 2026 is a Monday: the day the task was due on.
        let weekdays = keys(&stats.by_weekday);
        assert_eq!(weekdays, [("fri".into(), 1), ("mon".into(), 1), ("tue".into(), 1), ("wed".into(), 1)]);
    }

    #[test]
    fn the_evaluation_keeps_to_the_lists_and_the_span_asked_for() {
        let entries = vec![moved("a", 16, 17), Postponement { collection: "work".into(), ..moved("b", 18, 19) }];
        let home = HashSet::from(["home".to_string()]);
        let stats = stats(&entries, None, Some(&home), &[]);
        assert_eq!((stats.total, stats.tasks.len()), (1, 1));
        assert!(stats.by_list.iter().all(|g| g.key == "home"), "nothing of other lists, not even its id");
        assert_eq!(super::stats(&entries, Some(day(17)), None, &[]).total, 1);
    }

    #[tokio::test]
    async fn the_history_is_kept_a_year_in_a_private_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("state").join(FILE);
        let history = Postponements::new(Some(file.clone()));
        let old = Postponement { at: at(20) - Duration::days(366), ..moved("old", 1, 2) };
        history.record(vec![old, moved("a", 16, 17)], at(20), YEAR).await.expect("recorded");
        let again = Postponements::new(Some(file.clone()));
        assert_eq!(again.list(at(20), YEAR).await.expect("read"), vec![moved("a", 16, 17)], "the old one is gone");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).expect("written").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let text = std::fs::read_to_string(&file).expect("written");
        assert!(!text.contains("Task a"), "no title in the file");
    }

    #[tokio::test]
    async fn a_lowered_retention_drops_the_older_ones_at_once_and_a_raised_one_keeps_them() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join(FILE);
        let history = Postponements::new(Some(file.clone()));
        let days_ago = |n: i64| Postponement { at: at(20) - Duration::days(n), ..moved(&format!("t{n}"), 16, 17) };
        let three_years = PostponeRetention::clamped(1095);
        history.record(vec![days_ago(400), days_ago(100), days_ago(10)], at(20), three_years).await.expect("recorded");
        assert_eq!(history.list(at(20), three_years).await.expect("read").len(), 3, "all within three years");

        // Lowered to 90 days: the two older ones go now, also from the file.
        let quarter = PostponeRetention::clamped(90);
        assert_eq!(history.tidy(at(20), quarter).await.expect("tidied"), 2);
        let again = Postponements::new(Some(file));
        assert_eq!(again.list(at(20), three_years).await.expect("read"), vec![days_ago(10)], "gone for good");
        assert_eq!(again.tidy(at(20), quarter).await.expect("nothing to do"), 0);

        // Below the span it is a month, not less.
        let month = PostponeRetention::clamped(1);
        let kept = history.list(at(20), month).await.expect("read");
        assert_eq!(kept, vec![days_ago(10)], "ten days are within the 30 kept at least");
    }

    #[tokio::test]
    async fn clearing_removes_only_the_lists_named() {
        let history = Postponements::new(None);
        let work = Postponement { collection: "work".into(), ..moved("b", 18, 19) };
        history.record(vec![moved("a", 16, 17), work.clone()], at(20), YEAR).await.expect("recorded");
        let home = HashSet::from(["home".to_string()]);
        assert_eq!(history.clear(Some(&home)).await.expect("cleared"), 1);
        assert_eq!(history.list(at(20), YEAR).await.expect("read"), vec![work]);
        assert_eq!(history.clear(None).await.expect("cleared"), 1);
        assert!(history.list(at(20), YEAR).await.expect("read").is_empty());
    }

    #[tokio::test]
    async fn a_broken_file_is_refused_not_overwritten() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join(FILE);
        std::fs::write(&file, "not json").expect("written");
        let history = Postponements::new(Some(file.clone()));
        assert!(matches!(
            history.record(vec![moved("a", 16, 17)], at(20), YEAR).await,
            Err(PostponementsError::Parse { .. })
        ));
        assert_eq!(std::fs::read_to_string(&file).expect("kept"), "not json");
    }
}
