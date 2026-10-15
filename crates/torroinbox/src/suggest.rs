//! "AI task" and "AI event" (issue #120, task E8.24 in `docs/tasks/eingang.md`): an AI reads an
//! entry and suggests a task or an event; the dialog of "Make task" or "Make event" opens with it
//! filled in, and nothing is saved before the person presses "Create".
//!
//! - **What the window asks the core** ([`SuggestSource`]): whether an AI is set up
//!   (`assistant.list_engines` lists an AI account without a problem), the suggestions
//!   (`inbox.suggest_task_from_item`, `inbox.suggest_event_from_item`), and making the task or event
//!   with the note and place the dialog holds (`inbox.create_task_from_item`,
//!   `inbox.create_event_from_item`). The core picks the AI, keeps mail with local models and reads the
//!   answer; the window only shows it.
//! - **The buttons** ([`ai_buttons`]): beside "Make task" / "Task" in the panel and the reader. Since
//!   issue #168 also on rows and cards ([`row_action`]): "AI task" right after "Task", "AI event" right
//!   after "Event" wherever those stand, so each AI button has its plain one beside it. Without an AI
//!   they are off and faded, their tooltip says "Set up an AI first".
//! - **Into the dialog** ([`due_of`], [`event_times`]): the due day of a task (a time of day is
//!   dropped: the dialog makes tasks due on a day), the start and length of an event in local time.
//!   A whole-day event becomes that day at 09:00 for an hour: the dialog makes timed events only.

use std::future::Future;

use chrono::{NaiveDate, NaiveTime, TimeDelta, Utc};
use iced::Element;
use torro_core::i18n::tr;
use torro_gui::datetime;
use torro_gui::icon::Icon;
use torro_gui::widgets::{self, RowAction};
use torro_inbox::demo::DemoSource;
use torro_inbox::{InboxError, InboxSource, ItemFilter};
use torro_ipc::inbox::{EventFields, InboxClient};
use torro_ipc::{EventSuggestion, TaskSuggestion};
use torro_pim::EventTime;

use crate::app::Message;

/// What the AI buttons ask the core for (issue #120). The core's client sends the requests; the demo
/// answers them from its own data; the tests' empty source has no AI.
pub trait SuggestSource: Send + Sync {
    /// Whether an AI is set up for the buttons: an AI account without a known problem. `false` when
    /// it cannot be asked; the buttons are off then.
    fn ai_ready(&self) -> impl Future<Output = bool> + Send;

    /// The AI's task for the entry `id` (`inbox.suggest_task_from_item`); nothing is saved.
    /// [`InboxError::Other`] carries the core's words (no AI, the AI's server failed …).
    fn suggest_task(&self, id: String) -> impl Future<Output = Result<TaskSuggestion, InboxError>> + Send;

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

    async fn suggest_task(&self, id: String) -> Result<TaskSuggestion, InboxError> {
        self.suggest_task_from_item(id).await
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
/// entry's time), and an event that day at 10:00 for 45 minutes in "Room 2" with the sender. Making
/// either marks the entry done, as the core does; the note and place are kept nowhere.
impl SuggestSource for DemoSource {
    async fn ai_ready(&self) -> bool {
        true
    }

    async fn suggest_task(&self, id: String) -> Result<TaskSuggestion, InboxError> {
        let (item, tomorrow) = demo_entry(self, &id).await?;
        Ok(TaskSuggestion {
            title: format!("Follow up: {}", crate::list::one_line(&item.title)),
            notes: item.body.as_deref().map(crate::list::one_line).filter(|body| !body.is_empty()),
            due: Some(EventTime::Date(tomorrow)),
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

    async fn suggest_task(&self, _id: String) -> Result<TaskSuggestion, InboxError> {
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

/// Which suggestion a button asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    Task,
    Event,
}

impl Wanted {
    /// The button's word: "AI task", "AI event".
    pub fn label(self) -> &'static str {
        match self {
            Wanted::Task => tr("AI task"),
            Wanted::Event => tr("AI event"),
        }
    }

    /// What the button sends for the entry `id`.
    pub fn message(self, id: &str) -> Message {
        match self {
            Wanted::Task => Message::SuggestTask(id.to_owned()),
            Wanted::Event => Message::SuggestEvent(id.to_owned()),
        }
    }
}

/// The tooltip of an AI button: its word, and while no AI is set up (`ready` false) that it needs one
/// first – "AI task · Set up an AI first".
pub fn tooltip(wanted: Wanted, ready: bool) -> String {
    if ready { wanted.label().to_owned() } else { format!("{} · {}", wanted.label(), tr("Set up an AI first")) }
}

/// The AI button for `wanted` on the entry `id` as an icon (the panel's footer): [`Icon::Sparkles`],
/// off with its [`tooltip`] while no AI is set up.
pub fn icon_button<'a>(wanted: Wanted, id: &str, ready: bool) -> Element<'a, Message> {
    widgets::icon_button(Icon::Sparkles, tooltip(wanted, ready), ready.then(|| wanted.message(id)))
}

/// Whether `message` is what an AI button sends (issue #168): on a card such a button is the
/// companion of its plain one – always its icon alone, never weighed when the card's buttons choose
/// between words and icons (`widgets::bar_words`), so a card keeps its words as before.
pub fn is_ai(message: &Message) -> bool {
    matches!(message, Message::SuggestTask(_) | Message::SuggestEvent(_))
}

/// The AI button for `wanted` on the entry `id` in a row's action bar or a card's buttons (issue
/// #168): [`Icon::Sparkles`] with "AI task" or "AI event", no key of its own; while no AI is set up
/// (`ready` false) off with the reason "Set up an AI first" (`widgets::RowAction::blocked`).
pub fn row_action<'a>(wanted: Wanted, id: &str, ready: bool) -> RowAction<'a, Message> {
    let action = RowAction::new(Icon::Sparkles, wanted.label(), "", wanted.message(id));
    if ready { action } else { action.blocked(tr("Set up an AI first")) }
}

/// The AI buttons of an entry in the order they stand beside "Make task" and "Make event": task, then
/// event – each the message it sends (`None` while no AI is set up) and its tooltip.
pub fn ai_buttons(id: &str, ready: bool) -> [(Wanted, Option<Message>, String); 2] {
    [Wanted::Task, Wanted::Event].map(|wanted| (wanted, ready.then(|| wanted.message(id)), tooltip(wanted, ready)))
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

    /// Acceptance of issue #120: without an AI both buttons are off and say why.
    #[test]
    fn without_an_ai_the_buttons_are_off_and_say_why() {
        let off = ai_buttons("e1", false);
        assert!(off.iter().all(|(_, message, _)| message.is_none()));
        assert_eq!(off[0].2, format!("{} · {}", tr("AI task"), tr("Set up an AI first")));
        assert_eq!(off[1].2, format!("{} · {}", tr("AI event"), tr("Set up an AI first")));
        let on = ai_buttons("e1", true);
        assert!(matches!(&on[0].1, Some(Message::SuggestTask(id)) if id == "e1"));
        assert!(matches!(&on[1].1, Some(Message::SuggestEvent(id)) if id == "e1"));
        assert_eq!((on[0].2.as_str(), on[1].2.as_str()), (tr("AI task"), tr("AI event")));
    }

    /// Issue #168: on rows and cards the AI buttons are there with or without an AI; without one they
    /// send nothing and say why.
    #[test]
    fn row_actions_of_the_ai_are_off_without_one() {
        let off = row_action(Wanted::Task, "e1", false);
        assert!(off.press().is_none());
        assert_eq!(off.tip(), tooltip(Wanted::Task, false));
        let on = row_action(Wanted::Event, "e1", true);
        assert!(matches!(on.press(), Some(Message::SuggestEvent(id)) if id == "e1"));
        assert_eq!((on.icon, on.key), (Icon::Sparkles, ""));
        assert!(is_ai(&off.on_press) && is_ai(&on.on_press));
        assert!(!is_ai(&Message::Refresh), "only the AI buttons");
    }
}
