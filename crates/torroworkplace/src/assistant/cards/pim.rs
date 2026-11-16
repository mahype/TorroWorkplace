//! The event and task cards (issue #797, `docs/inhaltskarten.md` "Termin", "Aufgabe"): an item the assistant saved
//! or found, as the calendar and the task lists hold it now.
//!
//! Both are read from the core's cache (for events the days it holds) with the rights of the conversation's client
//! ([`Lookup::reach`]): a collection outside its selection is not there, and neither are its items. Nothing here
//! logs a title.

use chrono::{DateTime, NaiveTime, Utc};
use torro_ipc::Request;
use torro_ipc::api::Rights;
use torro_ipc::assistant::{Card, Overlap};
use torro_pim::model::EventStatus;
use torro_pim::{Collection, Event, EventTime, Transparency};

use super::Lookup;
use crate::pim::{PimService, Reach};

/// What the event `uid` (one occurrence of a series with `recurrence_id`) shows now: [`Card::Event`] with the event,
/// its calendar's name and colour, and at most two other events at the same time in the calendars the conversation's
/// client may read (`overlaps`, the earliest first), the rest counted in `more_overlaps`.
///
/// Looks only in calendars within [`Lookup::rights`] (a calendar out of reach is not there, its events neither as the
/// event nor as an overlap), in the days the core caches; without `recurrence_id` for a series the next occurrence
/// from now ([`chosen`]); answers [`Card::Unavailable`] for an event gone or out of reach, and when the client may not
/// list events at all. What counts as an overlap: [`overlaps`]. Logs no title.
pub async fn look_event(lookup: &Lookup<'_>, uid: &str, recurrence_id: Option<&EventTime>) -> Card {
    event_card(lookup.pim, lookup.rights, uid, recurrence_id, Utc::now()).await
}

/// [`look_event`] on `pim` with `rights` at `now`: `calendar.list_events` as the client would send it, then the
/// event and the events at its time from the cache.
async fn event_card(
    pim: &PimService,
    rights: &Rights,
    uid: &str,
    recurrence_id: Option<&EventTime>,
    now: DateTime<Utc>,
) -> Card {
    if !rights.allows_request(&Request::Events { start: now, end: now }) {
        return Card::Unavailable;
    }
    let reach = Reach::Client(rights);
    let events = pim.cached_events_within(reach).await;
    let calendars = pim.calendars_within(reach).await;
    shown_event(&events, &calendars, uid, recurrence_id, now)
}

/// The card of the event `uid` among `events` (all in reach) with its calendar from `calendars`.
fn shown_event(
    events: &[Event],
    calendars: &[Collection],
    uid: &str,
    recurrence_id: Option<&EventTime>,
    now: DateTime<Utc>,
) -> Card {
    let Some(event) = chosen(events, uid, recurrence_id, now) else { return Card::Unavailable };
    let Some(calendar) = calendars.iter().find(|calendar| calendar.id == event.collection) else {
        return Card::Unavailable;
    };
    let mut others: Vec<&Event> = events.iter().filter(|other| overlaps(event, other)).collect();
    others.sort_by(|a, b| (span(a), &a.summary).cmp(&(span(b), &b.summary)));
    let more = others.len().saturating_sub(SHOWN_OVERLAPS);
    let overlaps = others
        .into_iter()
        .take(SHOWN_OVERLAPS)
        .map(|other| Overlap { summary: other.summary.clone(), start: other.start, end: other.end })
        .collect();
    Card::Event {
        event: Box::new(event.clone()),
        calendar: calendar.name.clone(),
        color: calendar.color.clone(),
        overlaps,
        more_overlaps: u32::try_from(more).unwrap_or(u32::MAX),
    }
}

/// What the task `uid` shows now: [`Card::Todo`] with the task and its list's name and colour.
///
/// Looks only in task lists within [`Lookup::rights`]; answers [`Card::Unavailable`] for a task gone or out of
/// reach, and when the client may not list tasks at all. Logs no title.
pub async fn look_todo(lookup: &Lookup<'_>, uid: &str) -> Card {
    todo_card(lookup.pim, lookup.rights, uid).await
}

/// [`look_todo`] on `pim` with `rights`: `tasks.list_todos` as the client would send it, then the task by its uid.
async fn todo_card(pim: &PimService, rights: &Rights, uid: &str) -> Card {
    if !rights.allows_request(&Request::Todos) {
        return Card::Unavailable;
    }
    let reach = Reach::Client(rights);
    let Some(todo) = pim.todos_within(reach).await.into_iter().find(|todo| todo.uid == uid) else {
        return Card::Unavailable;
    };
    // The list is in reach, or the task would not have been listed; a list gone in between is out of reach too.
    let lists = pim.task_lists_within(reach).await;
    let Some(list) = lists.into_iter().find(|list| list.id == todo.collection) else {
        return Card::Unavailable;
    };
    Card::Todo { todo: Box::new(todo), list: list.name, color: list.color }
}

/// How many overlaps an event card names; the rest it counts (`docs/assistent.md`, "Überschneidungen werden immer
/// benannt": the first two and "+ n more").
const SHOWN_OVERLAPS: usize = 2;

/// The occurrence of the event `uid` a card shows: with `recurrence_id` exactly that one; without, the single event,
/// or of a series the first occurrence not ended by `now` (one running counts), else its last. `None` when none is
/// there.
fn chosen<'a>(
    events: &'a [Event],
    uid: &str,
    recurrence_id: Option<&EventTime>,
    now: DateTime<Utc>,
) -> Option<&'a Event> {
    let mut found: Vec<&Event> = events.iter().filter(|event| event.uid == uid).collect();
    if let Some(key) = recurrence_id {
        return found.into_iter().find(|event| event.recurrence_id.as_ref() == Some(key));
    }
    found.sort_by_key(|event| span(event));
    let ended = |event: &&Event| span(event).1 <= now;
    found.iter().find(|event| !ended(event)).or(found.last()).copied()
}

/// `other` keeps the same time busy as `event`: both are timed (an all-day event takes no hours), keep their time
/// busy (opaque and not cancelled) and their times cross; never an occurrence of the event's own series.
fn overlaps(event: &Event, other: &Event) -> bool {
    let busy = |event: &Event| {
        matches!(event.start, EventTime::DateTime(_))
            && event.transparency == Transparency::Opaque
            && event.status != Some(EventStatus::Cancelled)
    };
    let ((start, end), (other_start, other_end)) = (span(event), span(other));
    other.uid != event.uid && busy(event) && busy(other) && other_start < end && start < other_end
}

/// When `event` starts and ends as instants; an all-day event from midnight UTC of its first day, an event without
/// end ends where it starts. Only for ordering and for timed events' overlaps, where the days never matter.
fn span(event: &Event) -> (DateTime<Utc>, DateTime<Utc>) {
    let instant = |time: &EventTime| match time {
        EventTime::DateTime(at) => *at,
        EventTime::Date(day) => day.and_time(NaiveTime::MIN).and_utc(),
    };
    let start = instant(&event.start);
    (start, event.end.as_ref().map_or(start, instant))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use torro_ipc::api::{Access, Area, AreaRights, Level, Selection};
    use torro_pim::{CollectionKind, Todo};

    use super::*;
    use crate::settings_service::SettingsStore;

    const HOME: &str = "nextcloud:anna@cloud.example.org/https://cloud.example.org/remote.php/dav/calendars/anna/home/";
    const WORK: &str = "nextcloud:anna@cloud.example.org/https://cloud.example.org/remote.php/dav/calendars/anna/work/";

    fn list(id: &str, name: &str) -> Collection {
        Collection {
            id: id.into(),
            name: name.into(),
            kind: CollectionKind::Todos,
            color: Some("#3FB984".into()),
            writable: true,
            source: None,
        }
    }

    fn todo(uid: &str, collection: &str) -> Todo {
        Todo {
            uid: uid.into(),
            collection: collection.into(),
            href: Some(format!("{collection}{uid}.ics")),
            etag: Some("\"1\"".into()),
            summary: "Water plants".into(),
            due: None,
            priority: None,
            completed: false,
            parent: None,
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
            focus: Default::default(),
        }
    }

    /// A service without settings file and network, caching the lists Home and Work with one task each.
    async fn service() -> PimService {
        let pim = PimService::with_settings(SettingsStore::without_file());
        pim.cache_collections(vec![list(HOME, "Home"), list(WORK, "Work")]).await;
        pim.cache_todos(vec![todo("plants", HOME), todo("report", WORK)]).await;
        pim
    }

    /// The assistant's rights (tasks readable) on the task lists `lists` only.
    fn rights(lists: &[&str]) -> Rights {
        let task_lists = Selection::Only(lists.iter().map(|id| id.to_string()).collect());
        Rights { task_lists, ..Rights::assistant() }
    }

    #[tokio::test]
    async fn a_task_shows_with_its_list_while_the_client_reaches_it() {
        let pim = service().await;
        let card = todo_card(&pim, &rights(&[HOME, WORK]), "plants").await;
        let Card::Todo { todo, list, color } = card else { panic!("a task card, got {card:?}") };
        assert_eq!((todo.uid.as_str(), list.as_str(), color.as_deref()), ("plants", "Home", Some("#3FB984")));
    }

    #[tokio::test]
    async fn a_task_out_of_reach_or_gone_is_unavailable() {
        let pim = service().await;
        assert_eq!(todo_card(&pim, &rights(&[HOME]), "report").await, Card::Unavailable, "its list is not selected");
        assert_eq!(todo_card(&pim, &rights(&[HOME, WORK]), "gone").await, Card::Unavailable, "no such task");
        let mut no_tasks = rights(&[HOME, WORK]);
        no_tasks.areas.insert(Area::Tasks, AreaRights { level: Level::NoAccess, admin: false });
        assert!(!no_tasks.area(Area::Tasks).allows(Access::Read));
        assert_eq!(todo_card(&pim, &no_tasks, "plants").await, Card::Unavailable, "tasks may not be read");
        let mut denied = rights(&[HOME, WORK]);
        denied.denied.insert("tasks.list_todos".into());
        assert_eq!(todo_card(&pim, &denied, "plants").await, Card::Unavailable, "the command is switched off");
    }
    /// A calendar of anna's account.
    fn calendar(id: &str, name: &str) -> Collection {
        Collection { kind: CollectionKind::Events, color: Some("#3474E6".into()), ..list(id, name) }
    }

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 15, hour, minute, 0).single().expect("a time")
    }

    /// The event `uid` in `collection`, `summary`, from `start` to `end`.
    fn event(uid: &str, collection: &str, summary: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> Event {
        Event {
            uid: uid.into(),
            collection: collection.into(),
            href: Some(format!("{collection}{uid}.ics")),
            etag: Some("\"1\"".into()),
            summary: summary.into(),
            start: EventTime::DateTime(start),
            end: Some(EventTime::DateTime(end)),
            location: None,
            description: None,
            recurring: false,
            transparency: Transparency::Opaque,
            alarms: Vec::new(),
            recurrence_id: None,
            repeat: None,
            time_zone: None,
            url: None,
            status: None,
            geo: None,
            organizer: None,
            attendees: Vec::new(),
            exceptions: Vec::new(),
        }
    }

    /// Team sync 14:00–15:00 at HOME, and around it: three that cross it (one of them in WORK), one right after, one
    /// free, one cancelled, one all day.
    fn day() -> Vec<Event> {
        let mut free = event("free", HOME, "Focus", at(14, 0), at(15, 0));
        free.transparency = Transparency::Transparent;
        let mut cancelled = event("off", HOME, "Called off", at(14, 0), at(15, 0));
        cancelled.status = Some(EventStatus::Cancelled);
        let mut all_day = event("holiday", HOME, "Holiday", at(0, 0), at(0, 0));
        all_day.start = EventTime::Date(at(0, 0).date_naive());
        all_day.end = Some(EventTime::Date(at(0, 0).date_naive() + chrono::Days::new(1)));
        vec![
            event("sync", HOME, "Team sync", at(14, 0), at(15, 0)),
            event("call", HOME, "Call", at(14, 30), at(15, 0)),
            event("lunch", HOME, "Late lunch", at(13, 30), at(14, 15)),
            event("review", WORK, "Review", at(14, 45), at(16, 0)),
            event("after", HOME, "After", at(15, 0), at(16, 0)),
            free,
            cancelled,
            all_day,
        ]
    }

    fn calendars() -> Vec<Collection> {
        vec![calendar(HOME, "Home"), calendar(WORK, "Work")]
    }

    /// Three events cross Team sync: the card names the earliest two and counts the third; what only touches it, is
    /// free, cancelled or all day is no overlap.
    #[test]
    fn an_event_with_three_overlaps_names_two_and_counts_the_rest() {
        let card = shown_event(&day(), &calendars(), "sync", None, at(9, 0));
        let Card::Event { event, calendar, color, overlaps, more_overlaps } = card else { panic!("an event card") };
        assert_eq!((event.uid.as_str(), calendar.as_str(), color.as_deref()), ("sync", "Home", Some("#3474E6")));
        let named: Vec<&str> = overlaps.iter().map(|overlap| overlap.summary.as_str()).collect();
        assert_eq!(named, ["Late lunch", "Call"], "the earliest two");
        assert_eq!(overlaps[1].start, EventTime::DateTime(at(14, 30)));
        assert_eq!(more_overlaps, 1, "Review is counted");
        let Card::Event { overlaps, more_overlaps, .. } = shown_event(&day(), &calendars(), "holiday", None, at(9, 0))
        else {
            panic!("an event card")
        };
        assert!(overlaps.is_empty() && more_overlaps == 0, "an all-day event takes no hours");
    }

    /// Of a series without `recurrence_id` the next occurrence from now, a running one included; with it exactly that
    /// one, and a gone one is unavailable.
    #[test]
    fn a_series_shows_its_next_occurrence_or_the_one_named() {
        let occurrence = |hour| {
            let mut one = event("standup", HOME, "Standup", at(hour, 0), at(hour, 30));
            (one.recurring, one.recurrence_id) = (true, Some(one.start));
            one
        };
        let series = vec![occurrence(16), occurrence(8), occurrence(12)];
        let start = |card: Card| match card {
            Card::Event { event, .. } => event.start,
            other => panic!("an event card, got {other:?}"),
        };
        assert_eq!(start(shown_event(&series, &calendars(), "standup", None, at(12, 10))), occurrence(12).start);
        assert_eq!(start(shown_event(&series, &calendars(), "standup", None, at(13, 0))), occurrence(16).start);
        assert_eq!(start(shown_event(&series, &calendars(), "standup", None, at(20, 0))), occurrence(16).start);
        let named = EventTime::DateTime(at(8, 0));
        assert_eq!(start(shown_event(&series, &calendars(), "standup", Some(&named), at(20, 0))), named);
        let gone = EventTime::DateTime(at(10, 0));
        assert_eq!(shown_event(&series, &calendars(), "standup", Some(&gone), at(9, 0)), Card::Unavailable);
        assert_eq!(shown_event(&series, &calendars(), "nothing", None, at(9, 0)), Card::Unavailable);
    }

    /// From the cache with the client's rights: a calendar out of reach is not there, neither its events as the event
    /// nor as overlaps; without calendar rights there is no card.
    #[tokio::test]
    async fn an_event_out_of_reach_is_unavailable_and_overlaps_only_within_reach() {
        let pim = PimService::with_settings(SettingsStore::without_file());
        pim.cache_collections(calendars()).await;
        pim.cache_events(day()).await;
        let only_home = Rights { calendars: Selection::Only(vec![HOME.into()]), ..Rights::assistant() };
        let card = event_card(&pim, &only_home, "sync", None, at(9, 0)).await;
        let Card::Event { overlaps, more_overlaps, .. } = card else { panic!("an event card, got {card:?}") };
        assert_eq!((overlaps.len(), more_overlaps), (2, 0), "Review in Work is out of reach");
        assert_eq!(event_card(&pim, &only_home, "review", None, at(9, 0)).await, Card::Unavailable);
        let mut no_calendar = Rights::assistant();
        no_calendar.areas.insert(Area::Calendar, AreaRights { level: Level::NoAccess, admin: false });
        assert_eq!(event_card(&pim, &no_calendar, "sync", None, at(9, 0)).await, Card::Unavailable);
    }
}
