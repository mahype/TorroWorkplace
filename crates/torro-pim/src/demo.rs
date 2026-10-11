//! Sample data in memory, for building and trying the apps without a server.
//!
//! `torrocalendar --demo` and `torrotasks --demo` use it. Changes live until the app
//! closes. The sample content (task K.2 in `docs/tasks/kalender-aufgaben.md`) is built
//! around "today", so the week view is never empty.

use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Local, Utc};

use crate::source::PimSource;
use crate::{AccountState, Collection, Event, EventTime, ProviderError, Todo};

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

/// The sample content: collections (two calendars, two task lists, one with both),
/// events (task K.2) and todos (task K.2).
pub fn sample(today: chrono::NaiveDate) -> (Vec<Collection>, Vec<Event>, Vec<Todo>) {
    let _ = today;
    (Vec::new(), Vec::new(), Vec::new())
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
    async fn account(&self) -> Result<AccountState, ProviderError> {
        Ok(AccountState::Connected {
            server: "demo".into(),
            login_name: "demo".into(),
            last_sync: Some(Utc::now()),
            problem: None,
        })
    }

    async fn connect_nextcloud(&self, server: String) -> Result<String, ProviderError> {
        Err(ProviderError::Other(format!("demo mode cannot connect to {server}")))
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
                event.href = Some(format!("{}{}.ics", event.collection, event.uid));
                event.etag = Some(version);
                data.events.push(event.clone());
            }
            Some(href) => {
                let Some(slot) = data.events.iter_mut().find(|e| e.href.as_deref() == Some(href.as_str())) else {
                    return Err(ProviderError::Other(format!("no event at {href}")));
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
        if data.events.len() == before { Err(ProviderError::Other("no such event".into())) } else { Ok(()) }
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
                todo.href = Some(format!("{}{}.ics", todo.collection, todo.uid));
                todo.etag = Some(version);
                data.todos.push(todo.clone());
            }
            Some(href) => {
                let Some(slot) = data.todos.iter_mut().find(|t| t.href.as_deref() == Some(href.as_str())) else {
                    return Err(ProviderError::Other(format!("no task at {href}")));
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
        if data.todos.len() == before { Err(ProviderError::Other("no such task".into())) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CollectionKind;

    fn source_with_calendar() -> DemoSource {
        let source = DemoSource::default();
        source.lock().collections.push(Collection {
            id: "demo:/work/".into(),
            name: "Arbeit".into(),
            kind: CollectionKind::Events,
            color: Some("#3474E6".into()),
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
