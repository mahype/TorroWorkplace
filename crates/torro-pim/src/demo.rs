//! Sample data in memory, for building and trying the apps without a server.
//!
//! `torrocalendar --demo` and `torrotasks --demo` use it. Changes live until the app
//! closes. The sample content (task K.2 in `docs/tasks/kalender-aufgaben.md`) is built
//! around "today", so the week view is never empty. It spreads over two accounts (task M1.1),
//! a work and a home Nextcloud, so the apps can show calendars and lists grouped by account.

use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, Utc};

use crate::account::{account_of, local_id};
use crate::model::Link;
use crate::source::PimSource;
use crate::{AccountState, Collection, CollectionKind, Event, EventTime, PimAccount, ProviderError, Todo};

#[derive(Debug, Default)]
struct Data {
    collections: Vec<Collection>,
    events: Vec<Event>,
    todos: Vec<Todo>,
    next_id: u64,
}

/// See the module docs.
#[derive(Debug, Default)]
pub struct DemoSource {
    data: Mutex<Data>,
}

impl DemoSource {
    /// Sample calendars, events and tasks around `today`.
    pub fn new(today: chrono::NaiveDate) -> DemoSource {
        let (collections, events, todos) = sample(today);
        DemoSource { data: Mutex::new(Data { collections, events, todos, next_id: 0 }) }
    }

    fn lock(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The work account: the calendar "Work" and the list "Tasks".
pub const WORK_ACCOUNT: &str = "nextcloud:anna@work.example.org";
/// The home account: "Personal", "Groceries" and "Family".
pub const HOME_ACCOUNT: &str = "nextcloud:anna@home.example.org";
/// Both, in the order [`DemoSource::accounts`] lists them.
const ACCOUNTS: [&str; 2] = [WORK_ACCOUNT, HOME_ACCOUNT];

// Collection ids as the core writes them: `<account-id>/<collection URL>` (`crate::account`).
const WORK: &str = "nextcloud:anna@work.example.org/https://work.example.org/remote.php/dav/calendars/anna/work/";
const TASKS: &str = "nextcloud:anna@work.example.org/https://work.example.org/remote.php/dav/calendars/anna/tasks/";
const PERSONAL: &str =
    "nextcloud:anna@home.example.org/https://home.example.org/remote.php/dav/calendars/anna/personal/";
const GROCERIES: &str =
    "nextcloud:anna@home.example.org/https://home.example.org/remote.php/dav/calendars/anna/groceries/";
const FAMILY: &str = "nextcloud:anna@home.example.org/https://home.example.org/remote.php/dav/calendars/anna/family/";

/// The collection URL in a demo collection id, which item hrefs start with; an id of no demo
/// account (tests add their own) is its own URL.
fn url_of(collection: &str) -> &str {
    account_of(collection, &ACCOUNTS, |id| id).and_then(|account| local_id(collection, account)).unwrap_or(collection)
}

/// The sample content: five collections in two accounts, 27 events for this week and the two
/// after it, and 18 tasks. Dates are relative to `today`; a week runs Monday to Sunday.
pub fn sample(today: NaiveDate) -> (Vec<Collection>, Vec<Event>, Vec<Todo>) {
    let monday = today - Days::new(u64::from(today.weekday().num_days_from_monday()));
    let collections = vec![
        collection(WORK, "Work", CollectionKind::Events, "#3474E6"),
        collection(PERSONAL, "Personal", CollectionKind::Events, "#119C8E"),
        collection(TASKS, "Tasks", CollectionKind::Todos, "#4F9426"),
        collection(GROCERIES, "Groceries", CollectionKind::Todos, "#7A57E3"),
        collection(FAMILY, "Family", CollectionKind::Both, "#E0485D"),
    ];
    let mut events = sample_events(monday);
    for (n, item) in events.iter_mut().enumerate() {
        item.href = Some(format!("{}evt-{}.ics", url_of(&item.collection), n + 1));
        item.etag = Some(format!("\"sample-evt-{}\"", n + 1));
    }
    let mut todos = sample_todos(today);
    for (n, item) in todos.iter_mut().enumerate() {
        item.href = Some(format!("{}todo-{}.ics", url_of(&item.collection), n + 1));
        item.etag = Some(format!("\"sample-todo-{}\"", n + 1));
    }
    (collections, events, todos)
}

fn collection(id: &str, name: &str, kind: CollectionKind, color: &str) -> Collection {
    Collection { id: id.into(), name: name.into(), kind, color: Some(color.into()), writable: true }
}

/// Local wall-clock time on `day` as an instant. The sample avoids the hour that a DST
/// change skips.
fn at(day: NaiveDate, hour: u32, minute: u32) -> EventTime {
    let local = day.and_hms_opt(hour, minute, 0).and_then(|naive| naive.and_local_timezone(Local).earliest());
    EventTime::DateTime(local.map(|time| time.with_timezone(&Utc)).unwrap_or_default())
}

/// An event without href, etag and details. Set the rest with struct update syntax.
fn event(collection: &str, uid: &str, summary: &str, start: EventTime, end: EventTime) -> Event {
    Event {
        uid: uid.into(),
        collection: collection.into(),
        href: None,
        etag: None,
        summary: summary.into(),
        start,
        end: Some(end),
        location: None,
        description: None,
        recurring: false,
    }
}

/// A task without href, etag and details. Set the rest with struct update syntax.
fn todo(collection: &str, uid: &str, summary: &str, due: Option<EventTime>) -> Todo {
    Todo {
        uid: uid.into(),
        collection: collection.into(),
        href: None,
        etag: None,
        summary: summary.into(),
        due,
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

/// Events from Monday to Sunday of this week and of the two weeks after it.
fn sample_events(monday: NaiveDate) -> Vec<Event> {
    let day = |offset: u64| monday + Days::new(offset);
    let mut events = Vec::new();

    // The daily standup is one series: the same uid, one occurrence per weekday, read-only.
    for offset in [0, 1, 2, 3, 4, 7, 8, 9, 10, 11] {
        events.push(Event {
            recurring: true,
            ..event(WORK, "daily-standup@torro-demo", "Daily Standup", at(day(offset), 9, 30), at(day(offset), 9, 45))
        });
    }

    events.extend([
        Event {
            location: Some("Meeting room Elbe".into()),
            description: Some("Agree on goals and scope for the next sprint.".into()),
            ..event(WORK, "sprint-planning@torro-demo", "Sprint planning", at(day(0), 10, 0), at(day(0), 11, 30))
        },
        Event {
            location: Some("Room Rhine".into()),
            description: Some("Go through the booking API endpoints with the team.".into()),
            ..event(WORK, "api-workshop@torro-demo", "API design workshop", at(day(1), 13, 0), at(day(1), 15, 0))
        },
        // Overlaps the workshop above.
        Event {
            location: Some("Office 2.14".into()),
            ..event(WORK, "one-on-one-lena@torro-demo", "1:1 with Lena", at(day(1), 14, 30), at(day(1), 15, 0))
        },
        Event {
            location: Some("Canteen North".into()),
            ..event(WORK, "lunch-jana@torro-demo", "Lunch with Jana", at(day(2), 12, 30), at(day(2), 13, 30))
        },
        Event {
            location: Some("Video call".into()),
            description: Some("Interim status of the pilot project with the city council.".into()),
            ..event(WORK, "client-presentation@torro-demo", "Client presentation", at(day(3), 15, 0), at(day(3), 16, 0))
        },
        Event {
            location: Some("Room Elbe".into()),
            ..event(WORK, "retro@torro-demo", "Retrospective", at(day(4), 15, 0), at(day(4), 16, 0))
        },
        // Crosses midnight.
        Event {
            location: Some("Video call".into()),
            description: Some("Deploy version 2.4, then on call.".into()),
            ..event(WORK, "release-call@torro-demo", "Release call", at(day(4), 22, 30), at(day(5), 0, 30))
        },
        Event {
            location: Some("Room Rhine".into()),
            description: Some("Status of the pilot and dates for the autumn.".into()),
            ..event(WORK, "project-review@torro-demo", "Project review", at(day(9), 9, 0), at(day(9), 10, 0))
        },
        Event {
            location: Some("Room Elbe".into()),
            description: Some("Figures for the third quarter and outlook.".into()),
            ..event(WORK, "quarterly-review@torro-demo", "Quarterly review", at(day(16), 10, 0), at(day(16), 12, 0))
        },
        Event {
            location: Some("City park".into()),
            description: Some("Easy eight kilometres.".into()),
            ..event(PERSONAL, "run@torro-demo", "Run", at(day(1), 18, 30), at(day(1), 19, 30))
        },
        Event {
            location: Some("Indoor pool North".into()),
            ..event(PERSONAL, "swimming@torro-demo", "Swimming", at(day(3), 19, 0), at(day(3), 20, 0))
        },
        Event {
            location: Some("Forest sports field".into()),
            description: Some("Team training.".into()),
            ..event(PERSONAL, "football@torro-demo", "Football", at(day(5), 9, 30), at(day(5), 11, 0))
        },
        Event {
            location: Some("Dental practice Berger".into()),
            ..event(PERSONAL, "dentist@torro-demo", "Dentist", at(day(8), 8, 0), at(day(8), 8, 45))
        },
        event(FAMILY, "birthday-mum@torro-demo", "Mum's birthday", EventTime::Date(day(6)), EventTime::Date(day(7))),
        // All-day with an exclusive end: Tuesday to Thursday of next week.
        Event {
            location: Some("Baltic Sea".into()),
            description: Some("Holiday house in Kühlungsborn, arriving Tuesday morning.".into()),
            ..event(FAMILY, "vacation-baltic@torro-demo", "Vacation", EventTime::Date(day(8)), EventTime::Date(day(11)))
        },
        Event {
            location: Some("Grandma's".into()),
            description: Some("Grandma cooks, we bring dessert.".into()),
            ..event(FAMILY, "family-dinner@torro-demo", "Family dinner", at(day(13), 12, 0), at(day(13), 15, 0))
        },
        Event {
            location: Some("Kindergarten Sunshine".into()),
            description: Some("Topic: settling in for the new kindergarten year.".into()),
            ..event(
                FAMILY,
                "kindergarten-evening@torro-demo",
                "Kindergarten parents' evening",
                at(day(15), 19, 30),
                at(day(15), 21, 0),
            )
        },
    ]);
    events
}

/// Tasks due before, on and after `today` and some without a date, two parents with three
/// and two subtasks, three completed tasks, one with tags and one linked to a GitHub issue.
fn sample_todos(today: NaiveDate) -> Vec<Todo> {
    let before = |offset: u64| today - Days::new(offset);
    let after = |offset: u64| today + Days::new(offset);
    const QUARTERLY: &str = "quarterly-report@torro-demo";
    const WEEKLY_SHOP: &str = "weekly-shop@torro-demo";
    const VACATION: &str = "prepare-vacation@torro-demo";

    vec![
        Todo {
            priority: Some(1),
            description: Some("Sort receipts for 2025 by category, keep trade invoices separate.".into()),
            categories: vec!["Finance".into(), "Paperwork".into()],
            ..todo(TASKS, "tax-documents@torro-demo", "Sort tax documents", Some(at(before(4), 18, 0)))
        },
        Todo {
            priority: Some(5),
            description: Some("The detail should show tags and a link.".into()),
            url: Some("https://github.com/mahype/TorroWorkplace/issues/123".into()),
            link: Some(Link::github("mahype/TorroWorkplace", 123)),
            ..todo(TASKS, "github-issue@torro-demo", "Review task detail layout", Some(EventTime::Date(after(2))))
        },
        Todo {
            priority: Some(5),
            ..todo(TASKS, "invoices@torro-demo", "File invoices", Some(EventTime::Date(before(2))))
        },
        Todo {
            priority: Some(9),
            ..todo(TASKS, "bike@torro-demo", "Call the bike workshop", Some(EventTime::Date(after(3))))
        },
        Todo {
            priority: Some(5),
            description: Some("Outline is done, figures still missing.".into()),
            ..todo(TASKS, QUARTERLY, "Write quarterly report", Some(EventTime::Date(after(9))))
        },
        Todo {
            parent: Some(QUARTERLY.into()),
            completed: true,
            ..todo(TASKS, "quarterly-figures@torro-demo", "Pull figures from the CRM", Some(EventTime::Date(before(3))))
        },
        Todo {
            parent: Some(QUARTERLY.into()),
            ..todo(TASKS, "quarterly-outlook@torro-demo", "Write outlook", Some(EventTime::Date(after(7))))
        },
        Todo {
            parent: Some(QUARTERLY.into()),
            ..todo(TASKS, "quarterly-charts@torro-demo", "Insert charts", Some(EventTime::Date(after(8))))
        },
        Todo { completed: true, ..todo(TASKS, "passport@torro-demo", "Apply for passport", None) },
        todo(GROCERIES, "oats@torro-demo", "Oats", None),
        Todo { priority: Some(5), ..todo(GROCERIES, WEEKLY_SHOP, "Weekly shop", Some(EventTime::Date(after(1)))) },
        Todo {
            parent: Some(WEEKLY_SHOP.into()),
            ..todo(GROCERIES, "groceries-produce@torro-demo", "Vegetables and fruit", None)
        },
        Todo {
            parent: Some(WEEKLY_SHOP.into()),
            ..todo(GROCERIES, "groceries-bread@torro-demo", "Wholegrain bread", None)
        },
        Todo { parent: Some(WEEKLY_SHOP.into()), ..todo(GROCERIES, "groceries-yogurt@torro-demo", "Yogurt", None) },
        Todo { priority: Some(1), ..todo(FAMILY, "gift-mum@torro-demo", "Buy Mum's present", Some(at(today, 18, 0))) },
        Todo {
            priority: Some(5),
            description: Some("Sort everything out before the holiday house.".into()),
            ..todo(FAMILY, VACATION, "Prepare vacation", Some(EventTime::Date(after(1))))
        },
        Todo {
            parent: Some(VACATION.into()),
            completed: true,
            ..todo(FAMILY, "vacation-house@torro-demo", "Confirm holiday house", None)
        },
        Todo {
            parent: Some(VACATION.into()),
            ..todo(FAMILY, "vacation-tickets@torro-demo", "Book train tickets", Some(EventTime::Date(today)))
        },
        todo(FAMILY, "pediatrician@torro-demo", "Book pediatrician appointment", Some(EventTime::Date(after(10)))),
    ]
}

/// Start and end of an event as UTC instants; all-day dates count from local midnight.
fn span(event: &Event) -> (DateTime<Utc>, DateTime<Utc>) {
    let instant = |time: EventTime| match time {
        EventTime::DateTime(at) => at,
        EventTime::Date(date) => date
            .and_hms_opt(0, 0, 0)
            .and_then(|naive| naive.and_local_timezone(Local).earliest())
            .map(|local| local.with_timezone(&Utc))
            .unwrap_or_default(),
    };
    let start = instant(event.start);
    let end = event.end.map(instant).unwrap_or(start);
    (start, end)
}

impl PimSource for DemoSource {
    /// The work and the home account, both connected and just synced.
    async fn accounts(&self) -> Result<Vec<PimAccount>, ProviderError> {
        Ok(ACCOUNTS
            .iter()
            .map(|id| {
                let host = id.split_once('@').map_or(*id, |(_, host)| host);
                PimAccount {
                    id: (*id).into(),
                    label: format!("anna@{host}"),
                    kind: "nextcloud".into(),
                    state: AccountState::Connected {
                        server: format!("https://{host}"),
                        login_name: "anna".into(),
                        last_sync: Some(Utc::now()),
                        problem: None,
                    },
                }
            })
            .collect())
    }

    async fn connect_nextcloud(&self, server: String) -> Result<String, ProviderError> {
        Err(ProviderError::Other(format!("Demo mode cannot connect to {server}")))
    }

    async fn sync(&self) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn collections(&self) -> Result<Vec<Collection>, ProviderError> {
        Ok(self.lock().collections.clone())
    }

    async fn events(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Event>, ProviderError> {
        Ok(self
            .lock()
            .events
            .iter()
            .filter(|e| {
                let (s, e_end) = span(e);
                s < end && (e_end > start || (e_end == s && s >= start))
            })
            .cloned()
            .collect())
    }

    async fn todos(&self) -> Result<Vec<Todo>, ProviderError> {
        Ok(self.lock().todos.clone())
    }

    async fn save_event(&self, mut event: Event) -> Result<Event, ProviderError> {
        let mut data = self.lock();
        data.next_id += 1;
        let version = format!("\"demo-{}\"", data.next_id);
        match event.href.clone() {
            None => {
                if event.uid.is_empty() {
                    event.uid = format!("demo-event-{}", data.next_id);
                }
                event.href = Some(format!("{}{}.ics", url_of(&event.collection), event.uid));
                event.etag = Some(version);
                data.events.push(event.clone());
            }
            Some(href) => {
                let Some(slot) = data.events.iter_mut().find(|e| e.href.as_deref() == Some(href.as_str())) else {
                    return Err(ProviderError::Other(format!("No event at {href}")));
                };
                if slot.etag != event.etag {
                    return Err(ProviderError::Conflict { uid: event.uid });
                }
                event.etag = Some(version);
                *slot = event.clone();
            }
        }
        Ok(event)
    }

    async fn delete_event(&self, event: Event) -> Result<(), ProviderError> {
        let mut data = self.lock();
        let before = data.events.len();
        data.events.retain(|e| e.href != event.href || e.uid != event.uid);
        if data.events.len() == before { Err(ProviderError::Other("No such event".into())) } else { Ok(()) }
    }

    async fn save_todo(&self, mut todo: Todo) -> Result<Todo, ProviderError> {
        let mut data = self.lock();
        data.next_id += 1;
        let version = format!("\"demo-{}\"", data.next_id);
        match todo.href.clone() {
            None => {
                if todo.uid.is_empty() {
                    todo.uid = format!("demo-todo-{}", data.next_id);
                }
                todo.href = Some(format!("{}{}.ics", url_of(&todo.collection), todo.uid));
                todo.etag = Some(version);
                data.todos.push(todo.clone());
            }
            Some(href) => {
                let Some(slot) = data.todos.iter_mut().find(|t| t.href.as_deref() == Some(href.as_str())) else {
                    return Err(ProviderError::Other(format!("No task at {href}")));
                };
                if slot.etag != todo.etag {
                    return Err(ProviderError::Conflict { uid: todo.uid });
                }
                todo.etag = Some(version);
                *slot = todo.clone();
            }
        }
        Ok(todo)
    }

    async fn delete_todo(&self, todo: Todo) -> Result<(), ProviderError> {
        let mut data = self.lock();
        let before = data.todos.len();
        data.todos.retain(|t| t.href != todo.href || t.uid != todo.uid);
        if data.todos.len() == before { Err(ProviderError::Other("No such task".into())) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::Timelike;

    use super::*;

    /// A Sunday, the day the sample is written for.
    fn sunday() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 11).expect("valid date")
    }

    fn local_date(time: EventTime) -> NaiveDate {
        match time {
            EventTime::Date(date) => date,
            EventTime::DateTime(instant) => instant.with_timezone(&Local).date_naive(),
        }
    }

    /// Start and end of a timed event; `None` for all-day events and events without end.
    fn timed_bounds(event: &Event) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        match (event.start, event.end) {
            (EventTime::DateTime(start), Some(EventTime::DateTime(end))) => Some((start, end)),
            _ => None,
        }
    }

    #[test]
    fn collections_and_counts_per_collection() {
        let (collections, events, todos) = sample(sunday());
        let shape: Vec<(&str, CollectionKind)> = collections.iter().map(|c| (c.name.as_str(), c.kind)).collect();
        assert_eq!(
            shape,
            vec![
                ("Work", CollectionKind::Events),
                ("Personal", CollectionKind::Events),
                ("Tasks", CollectionKind::Todos),
                ("Groceries", CollectionKind::Todos),
                ("Family", CollectionKind::Both),
            ]
        );
        let events_in = |id: &str| events.iter().filter(|e| e.collection == id).count();
        assert_eq!(events_in(WORK), 19);
        assert_eq!(events_in(PERSONAL), 4);
        assert_eq!(events_in(FAMILY), 4);
        assert_eq!(events.len(), 27);
        let todos_in = |id: &str| todos.iter().filter(|t| t.collection == id).count();
        assert_eq!(todos_in(TASKS), 9);
        assert_eq!(todos_in(GROCERIES), 5);
        assert_eq!(todos_in(FAMILY), 5);
        assert_eq!(todos.len(), 19);

        for e in &events {
            assert!(collections.iter().any(|c| c.id == e.collection && c.kind.has_events()), "{}", e.summary);
        }
        for t in &todos {
            assert!(collections.iter().any(|c| c.id == t.collection && c.kind.has_todos()), "{}", t.summary);
        }
    }

    #[tokio::test]
    async fn the_sample_spreads_over_two_connected_accounts() {
        let accounts = DemoSource::new(sunday()).accounts().await.expect("accounts");
        let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, [WORK_ACCOUNT, HOME_ACCOUNT]);
        assert!(accounts.iter().all(|a| matches!(a.state, AccountState::Connected { .. })));
        assert_eq!(accounts[0].label, "anna@work.example.org");

        let (collections, _, _) = sample(sunday());
        let owner = |c: &Collection| account_of(&c.id, &accounts, |a| a.id.as_str()).map(|a| a.id.clone());
        let per_account = |id: &str| collections.iter().filter(|c| owner(c).as_deref() == Some(id)).count();
        assert_eq!((per_account(WORK_ACCOUNT), per_account(HOME_ACCOUNT)), (2, 3));
    }

    #[test]
    fn every_item_has_unique_href_and_etag() {
        let (_, events, todos) = sample(sunday());
        let mut hrefs = HashSet::new();
        for e in &events {
            let href = e.href.as_deref().expect("sample event has an href");
            assert!(href.starts_with(url_of(&e.collection)) && href.ends_with(".ics"), "{href}");
            assert!(e.etag.is_some(), "{href}");
            assert!(hrefs.insert(href.to_owned()), "duplicate href {href}");
        }
        for t in &todos {
            let href = t.href.as_deref().expect("sample task has an href");
            assert!(href.starts_with(url_of(&t.collection)) && href.ends_with(".ics"), "{href}");
            assert!(t.etag.is_some(), "{href}");
            assert!(hrefs.insert(href.to_owned()), "duplicate href {href}");
        }
    }

    #[test]
    fn daily_standup_is_one_recurring_series() {
        let (_, events, _) = sample(sunday());
        let standups: Vec<&Event> = events.iter().filter(|e| e.uid == "daily-standup@torro-demo").collect();
        assert_eq!(standups.len(), 10);
        assert_eq!(events.iter().filter(|e| e.recurring).count(), 10);
        let mut days: Vec<NaiveDate> = standups.iter().map(|e| local_date(e.start)).collect();
        days.sort();
        days.dedup();
        assert_eq!(days.len(), 10, "one occurrence per day");
        for e in standups {
            assert!(e.recurring);
            let (start, end) = timed_bounds(e).expect("standup is timed");
            let (start, end) = (start.with_timezone(&Local), end.with_timezone(&Local));
            assert_eq!((start.hour(), start.minute()), (9, 30));
            assert_eq!((end.hour(), end.minute()), (9, 45));
            assert!(start.weekday().number_from_monday() <= 5, "standup on a weekend");
        }
    }

    #[test]
    fn a_weekday_has_overlapping_meetings() {
        let (_, events, _) = sample(sunday());
        let timed: Vec<(&Event, DateTime<Utc>, DateTime<Utc>)> =
            events.iter().filter_map(|e| timed_bounds(e).map(|(s, t)| (e, s, t))).collect();
        let overlap = timed.iter().enumerate().any(|(i, (a, a_start, a_end))| {
            timed[i + 1..].iter().any(|(b, b_start, b_end)| {
                let day = local_date(a.start);
                a_start < b_end
                    && b_start < a_end
                    && day == local_date(b.start)
                    && day.weekday().number_from_monday() <= 5
            })
        });
        assert!(overlap);
    }

    #[test]
    fn all_day_vacation_spans_three_days() {
        let (_, events, _) = sample(sunday());
        let vacation = events.iter().find(|e| e.summary == "Vacation").expect("Vacation exists");
        assert!(vacation.all_day());
        let (EventTime::Date(first), Some(EventTime::Date(end))) = (vacation.start, vacation.end) else {
            panic!("Vacation is all-day with a date end");
        };
        assert_eq!((end - first).num_days(), 3, "end is exclusive");
        // Tuesday of next week: Sunday is the 6th day of this week.
        assert_eq!(first, sunday() + Days::new(2));
    }

    #[test]
    fn one_meeting_crosses_midnight() {
        let (_, events, _) = sample(sunday());
        let crosses = events.iter().filter_map(timed_bounds).any(|(start, end)| {
            let (start, end) = (start.with_timezone(&Local), end.with_timezone(&Local));
            start.date_naive() != end.date_naive() && (end.hour(), end.minute()) == (0, 30)
        });
        assert!(crosses);
    }

    #[test]
    fn subtasks_reference_existing_parents() {
        let (_, _, todos) = sample(sunday());
        let uids: HashSet<&str> = todos.iter().map(|t| t.uid.as_str()).collect();
        for t in &todos {
            if let Some(parent) = &t.parent {
                assert!(uids.contains(parent.as_str()), "{} has no parent {parent}", t.summary);
            }
        }
        let parents: HashSet<&str> = todos.iter().filter_map(|t| t.parent.as_deref()).collect();
        assert_eq!(parents.len(), 3);
        for parent in parents {
            let children = todos.iter().filter(|t| t.parent.as_deref() == Some(parent)).count();
            assert!((2..=3).contains(&children), "{parent} has {children} subtasks");
        }
    }

    #[test]
    fn tasks_cover_overdue_today_later_and_undated() {
        let (_, _, todos) = sample(sunday());
        let open = || todos.iter().filter(|t| !t.completed);
        assert_eq!(todos.iter().filter(|t| t.completed).count(), 3);
        assert!(open().any(|t| t.due.is_some_and(|due| local_date(due) < sunday())), "overdue");
        assert!(open().any(|t| t.due.is_some_and(|due| local_date(due) == sunday())), "due today");
        assert!(open().any(|t| t.due.is_some_and(|due| local_date(due) > sunday())), "later");
        assert!(todos.iter().any(|t| t.due.is_none()), "without due date");
        for priority in [Some(1), Some(5), Some(9), None] {
            assert!(todos.iter().any(|t| t.priority == priority), "priority {priority:?}");
        }
    }

    #[test]
    fn one_task_has_tags_and_one_a_link() {
        let (_, _, todos) = sample(sunday());
        assert!(todos.iter().any(|t| t.categories.len() > 1), "a task with tags");
        let linked: Vec<&Todo> = todos.iter().filter(|t| t.link.is_some()).collect();
        assert_eq!(linked.len(), 1);
        assert!(linked[0].link.as_ref().and_then(Link::github_ref).is_some(), "a GitHub link");
        assert!(linked[0].url.as_deref().is_some_and(|url| url.starts_with("https://github.com/")));
    }

    #[tokio::test]
    async fn week_query_returns_the_weeks_events() {
        // A Sunday and the Monday after it, so both edges of the week are covered.
        for today in [sunday(), sunday() + Days::new(1)] {
            let monday = today - Days::new(u64::from(today.weekday().num_days_from_monday()));
            let EventTime::DateTime(start) = at(monday, 0, 0) else { panic!("week starts at an instant") };
            let EventTime::DateTime(end) = at(monday + Days::new(7), 0, 0) else { panic!("week ends at an instant") };
            let got = DemoSource::new(today).events(start, end).await.expect("events");
            let (_, all, _) = sample(today);
            let week = monday..monday + Days::new(7);
            let expected: Vec<Event> = all.into_iter().filter(|e| week.contains(&local_date(e.start))).collect();
            assert!(!got.is_empty());
            assert_eq!(got, expected, "week of {monday}");
        }
    }

    fn source_with_calendar() -> DemoSource {
        let source = DemoSource::default();
        source.lock().collections.push(Collection {
            id: "demo:/work/".into(),
            name: "Work".into(),
            kind: CollectionKind::Events,
            color: Some("#3474E6".into()),
            writable: true,
        });
        source
    }

    #[tokio::test]
    async fn save_update_delete_round_trip() {
        let source = source_with_calendar();
        let start = Utc::now();
        let new = Event {
            uid: String::new(),
            collection: "demo:/work/".into(),
            href: None,
            etag: None,
            summary: "Workshop".into(),
            start: EventTime::DateTime(start),
            end: Some(EventTime::DateTime(start + chrono::Duration::hours(1))),
            location: None,
            description: None,
            recurring: false,
        };
        let mut saved = source.save_event(new).await.unwrap();
        assert!(saved.href.is_some() && saved.etag.is_some());
        let stale = saved.clone();
        saved.summary = "Workshop Teil 1".into();
        let updated = source.save_event(saved).await.unwrap();
        assert!(matches!(source.save_event(stale).await, Err(ProviderError::Conflict { .. })));
        let range = source.events(start - chrono::Duration::days(1), start + chrono::Duration::days(1)).await.unwrap();
        assert_eq!(range, vec![updated.clone()]);
        source.delete_event(updated).await.unwrap();
        assert!(
            source
                .events(start - chrono::Duration::days(1), start + chrono::Duration::days(1))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
