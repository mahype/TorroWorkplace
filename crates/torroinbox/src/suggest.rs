//! The AI fills in the dialogs of "Make task" and "Make event" (issue #120, task E8.24 in
//! `docs/tasks/eingang.md`): an AI reads an entry and suggests a task or an event; the person asks
//! for it with the sparkles in the dialog's head, and nothing is saved before they press "Create".
//!
//! - **What the window asks the core** ([`SuggestSource`]): whether an AI is set up
//!   (`assistant.list_engines` lists an AI account without a problem), the suggestions
//!   (`inbox.suggest_task_from_item`, `inbox.suggest_event_from_item`), and making the task or event
//!   with the note and place the dialog holds (`inbox.create_task_from_item`,
//!   `inbox.create_event_from_item`). The core picks the AI, keeps mail with local models and reads the
//!   answer; the window only shows it.
//! - **The button**: no AI button of its own beside "Task" and "Event" any more – "Task" opens the
//!   dialog, and the sparkles in its head ("Fill in with AI", `App::suggest_task`) ask the AI for
//!   the title, the note, the list (one of those the dialog offers) and the due day, the last only
//!   when the text names one. Without an AI the sparkles are off, their tooltip says "Set up an AI
//!   first".
//! - **Into the dialog** ([`due_of`], [`event_times`]): the due day of a task (a time of day is
//!   dropped: the dialog makes tasks due on a day), the start and length of an event in local time.
//!   A whole-day event becomes that day at 09:00 for an hour: the dialog makes timed events only.

use std::future::Future;

use chrono::{NaiveDate, NaiveTime, TimeDelta, Utc};
use torro_core::i18n::tr;
use torro_gui::datetime;
use torro_inbox::demo::DemoSource;
use torro_inbox::{InboxError, InboxSource, ItemFilter};
use torro_ipc::inbox::{EventFields, InboxClient};
use torro_ipc::{EventSuggestion, TaskSuggestion};
use torro_pim::EventTime;

/// What "Fill in with AI" asks the core for (issue #120). The core's client sends the requests; the demo
/// answers them from its own data; the tests' empty source has no AI.
pub trait SuggestSource: Send + Sync {
    /// Whether an AI is set up for "Fill in with AI": an AI account without a known problem. `false` when
    /// it cannot be asked; the buttons are off then.
    fn ai_ready(&self) -> impl Future<Output = bool> + Send;

    /// The AI's task for the entry `id` (`inbox.suggest_task_from_item`), its list one of `lists`
    /// (the names the dialog offers) or none; nothing is saved. [`InboxError::Other`] carries the
    /// core's words (no AI, the AI's server failed …).
    fn suggest_task(
        &self,
        id: String,
        lists: Vec<String>,
    ) -> impl Future<Output = Result<TaskSuggestion, InboxError>> + Send;

    /// The AI's event for the entry `id` (`inbox.suggest_event_from_item`); nothing is saved.
    fn suggest_event(&self, id: String) -> impl Future<Output = Result<EventSuggestion, InboxError>> + Send;

    /// `inbox.create_task_from_item` with `notes` before the entry's text (`None`: the entry's text
    /// alone) and the day as due date; the entry is done then.
    fn create_task(
        &self,
        id: String,
        list: String,
        title: Option<String>,
        due: Option<NaiveDate>,
        notes: Option<String>,
    ) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// `inbox.create_event_from_item` with the place and the note of `fields` too; the entry is done
    /// then.
    fn create_event(
        &self,
        id: String,
        calendar: String,
        fields: EventFields,
    ) -> impl Future<Output = Result<(), InboxError>> + Send;
}

impl SuggestSource for InboxClient {
    async fn ai_ready(&self) -> bool {
        InboxClient::ai_ready(self).await
    }

    async fn suggest_task(&self, id: String, lists: Vec<String>) -> Result<TaskSuggestion, InboxError> {
        self.suggest_task_from_item(id, lists).await
    }

    async fn suggest_event(&self, id: String) -> Result<EventSuggestion, InboxError> {
        self.suggest_event_from_item(id).await
    }

    async fn create_task(
        &self,
        id: String,
        list: String,
        title: Option<String>,
        due: Option<NaiveDate>,
        notes: Option<String>,
    ) -> Result<(), InboxError> {
        self.create_task_with(id, list, title, due, notes).await
    }

    async fn create_event(&self, id: String, calendar: String, fields: EventFields) -> Result<(), InboxError> {
        self.create_event_with(id, calendar, fields).await
    }
}

/// The demo has an AI: it suggests to follow the entry up the day after its "now" (the newest
/// entry's time) in the list whose name its text names (else the last one offered, to show the
/// choice), and an event that day at 10:00
/// for 45 minutes in "Room 2" with the sender. Making
/// either marks the entry done, as the core does; the note and place are kept nowhere.
impl SuggestSource for DemoSource {
    async fn ai_ready(&self) -> bool {
        true
    }

    async fn suggest_task(&self, id: String, lists: Vec<String>) -> Result<TaskSuggestion, InboxError> {
        let (item, tomorrow) = demo_entry(self, &id).await?;
        let text = format!("{} {}", item.title, item.body.as_deref().unwrap_or_default()).to_lowercase();
        Ok(TaskSuggestion {
            title: format!("Follow up: {}", crate::list::one_line(&item.title)),
            notes: item.body.as_deref().map(crate::list::one_line).filter(|body| !body.is_empty()),
            due: Some(EventTime::Date(tomorrow)),
            list: lists.iter().find(|list| text.contains(&list.to_lowercase())).or(lists.last()).cloned(),
            engine: "ai:ollama@localhost:11434".into(),
            model: "qwen3:8b".into(),
        })
    }

    async fn suggest_event(&self, id: String) -> Result<EventSuggestion, InboxError> {
        let (item, tomorrow) = demo_entry(self, &id).await?;
        let at = |hour: u32, minute: u32| {
            NaiveTime::from_hms_opt(hour, minute, 0)
                .map(|time| EventTime::DateTime(datetime::to_utc(tomorrow.and_time(time))))
        };
        Ok(EventSuggestion {
            title: crate::list::one_line(&item.title),
            start: at(10, 0),
            end: at(10, 45),
            location: Some("Room 2".into()),
            attendees: item.from.map(|from| from.name).into_iter().collect(),
            engine: "ai:ollama@localhost:11434".into(),
            model: "qwen3:8b".into(),
        })
    }

    async fn create_task(
        &self,
        id: String,
        list: String,
        title: Option<String>,
        due: Option<NaiveDate>,
        _notes: Option<String>,
    ) -> Result<(), InboxError> {
        self.create_task_from_item(id, list, title, due).await
    }

    async fn create_event(&self, id: String, calendar: String, fields: EventFields) -> Result<(), InboxError> {
        let EventFields { title, start, end, .. } = fields;
        crate::hub::HubSource::create_event_from_item(self, id, calendar, title, start, end).await
    }
}

/// The demo's entry `id` and the local day after its "now" (the newest entry's time).
async fn demo_entry(demo: &DemoSource, id: &str) -> Result<(torro_inbox::Item, NaiveDate), InboxError> {
    let all = demo.list_items(ItemFilter::default()).await?;
    let now = all.iter().map(|item| item.updated).max().unwrap_or_else(Utc::now);
    let item = all.into_iter().find(|item| item.id == id).ok_or(InboxError::NotFound)?;
    let tomorrow = datetime::to_local(now).date() + chrono::Days::new(1);
    Ok((item, tomorrow))
}

#[cfg(test)]
impl SuggestSource for crate::empty::EmptySource {
    async fn ai_ready(&self) -> bool {
        false
    }

    async fn suggest_task(&self, _id: String, _lists: Vec<String>) -> Result<TaskSuggestion, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn suggest_event(&self, _id: String) -> Result<EventSuggestion, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn create_task(
        &self,
        _id: String,
        _list: String,
        _title: Option<String>,
        _due: Option<NaiveDate>,
        _notes: Option<String>,
    ) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }

    async fn create_event(&self, _id: String, _calendar: String, _fields: EventFields) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }
}

/// The local day a task is due, as the dialog keeps it: the day of a date, the local day of an
/// instant.
pub fn due_of(due: Option<EventTime>) -> Option<NaiveDate> {
    match due? {
        EventTime::Date(day) => Some(day),
        EventTime::DateTime(at) => Some(datetime::to_local(at).date()),
    }
}

/// The local day, time and minutes of an event the AI suggested, as the dialog holds them; `None`
/// when it named no day. A whole day starts at 09:00 for 60 minutes (module docs); a time without an
/// end keeps `default_minutes`.
pub fn event_times(suggestion: &EventSuggestion, default_minutes: i64) -> Option<(NaiveDate, NaiveTime, i64)> {
    match (suggestion.start?, suggestion.end) {
        (EventTime::Date(day), _) => Some((day, NaiveTime::from_hms_opt(9, 0, 0)?, 60)),
        (EventTime::DateTime(start), end) => {
            let local = datetime::to_local(start);
            let minutes = match end {
                Some(EventTime::DateTime(end)) if end > start => (end - start).num_minutes(),
                _ => default_minutes,
            };
            Some((local.date(), local.time(), minutes.clamp(1, TimeDelta::days(1).num_minutes())))
        }
    }
}

/// The note the dialog of an event starts with: who takes part ("With: Kim, Anna"), which the
/// event has no field for. `None` without anybody.
pub fn attendees_note(suggestion: &EventSuggestion) -> Option<String> {
    (!suggestion.attendees.is_empty()).then(|| format!("{} {}", tr("With:"), suggestion.attendees.join(", ")))
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).expect("valid time").with_timezone(&Utc)
    }

    fn event(start: Option<EventTime>, end: Option<EventTime>) -> EventSuggestion {
        EventSuggestion {
            title: "Lunch".into(),
            start,
            end,
            location: None,
            attendees: vec!["Kim".into(), "Anna".into()],
            engine: "ai:x".into(),
            model: "m".into(),
        }
    }

    #[test]
    fn suggested_times_become_the_dialogs() {
        let start = instant("2026-10-16T10:30:00Z");
        let local = datetime::to_local(start);
        let timed = event(Some(EventTime::DateTime(start)), Some(EventTime::DateTime(start + TimeDelta::minutes(90))));
        assert_eq!(event_times(&timed, 30), Some((local.date(), local.time(), 90)));
        let open = event(Some(EventTime::DateTime(start)), None);
        assert_eq!(event_times(&open, 30), Some((local.date(), local.time(), 30)), "no end: the dialog's length");
        let day = NaiveDate::from_ymd_opt(2026, 10, 20).expect("valid date");
        let whole = event(Some(EventTime::Date(day)), None);
        assert_eq!(event_times(&whole, 30), Some((day, NaiveTime::from_hms_opt(9, 0, 0).expect("time"), 60)));
        assert_eq!(event_times(&event(None, None), 30), None);
        assert_eq!(attendees_note(&timed), Some(format!("{} Kim, Anna", tr("With:"))));
        assert_eq!(due_of(Some(EventTime::Date(day))), Some(day));
        assert_eq!(due_of(Some(EventTime::DateTime(start))), Some(local.date()));
        assert_eq!(due_of(None), None);
    }
}
