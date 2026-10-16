//! Reminders of events (task ER1.3, `docs/erinnerungen.md`): which reminders there are, when they
//! fire, and what the person did with them.
//!
//! - **Rules** ([`plan`]) are pure, with "now" passed in: from the events of the calendars the
//!   suite shows, the setting `reminders` and the [`Ledger`] they list the reminders of the next
//!   [`ahead`], soonest first. An event's own reminders (`VALARM`, task ER1.1) come first; one
//!   without gets the default lead. A calendar set to "off" reminds of nothing; all-day and free
//!   events only when the setting says so. Whether the person accepted an event is not known yet
//!   (attendees come with task K.26), nor are work hours (K.33): those filters wait for them.
//! - **When a reminder is due:** from the moment it fires until its event ends, but one that was
//!   never shown is only due within [`grace`] after the later of its firing and its event's start.
//!   So after a sleep the reminder of a running meeting still comes ("Started 3 min ago"), while a
//!   core started in the afternoon does not bring back the morning; past events are past.
//! - **Style:** the reminder's own (`X-TORRO-STYLE`), else its calendar's. While another event
//!   with a call link runs and `never_during_call` is on, full screen becomes a notification
//!   (decision 3).
//! - **Ledger** (`<state>/reminders.json`, 0600): dismissed, snoozed and shown reminders and the
//!   pause, only on this computer (decision 1: dismissing never changes the event on the server).
//!   Entries of events that ended [`keep`] ago go.
//! - **Commands** ([`Reminders`]) with the contracts of task ER1.2: snooze and dismiss only what
//!   fired (`conflict`), 1 to 120 minutes, a pause of at most [`max_pause`], a preview at most
//!   once a minute per client (`busy`).
//! - **Run** ([`run`]): the core plans anew every minute and at each firing, and right after every
//!   command; a reminder that becomes due goes to the [`Sink`] once (again after a snooze ends).
//!   The sink files it in the inbox and shows it (tasks ER1.4, ER2.1).
//!
//! The id of a reminder is a checksum of calendar, uid, the occurrence's start and the trigger:
//! 32 hex characters, so the audit log takes it as it is. A moved event gives new ids.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_core::settings::{CalendarReminders, Reminders as Settings};
use torro_ipc::{Join, Reminder, ReminderState, Response, Snooze};
use torro_pim::{Alarm, AlarmAnchor, AlarmStyle, AlarmTrigger, Event, EventTime, Transparency};

/// The ledger's file in the core's state folder.
const FILE: &str = "reminders.json";

/// How far ahead `calendar.list_reminders` looks.
pub fn ahead() -> TimeDelta {
    TimeDelta::hours(36)
}

/// How long a reminder that was never shown stays due after it fired or its event started.
pub fn grace() -> TimeDelta {
    TimeDelta::minutes(15)
}

/// How long the ledger keeps what it knows of an event after the event ended.
pub fn keep() -> TimeDelta {
    TimeDelta::days(7)
}

/// The longest pause.
pub fn max_pause() -> TimeDelta {
    TimeDelta::hours(24)
}

/// How long a preview stays on the screen at most.
fn preview_for() -> TimeDelta {
    TimeDelta::minutes(10)
}

/// How often one client may ask for a preview.
fn preview_every() -> TimeDelta {
    TimeDelta::minutes(1)
}

/// The longest snooze, in minutes.
pub const MAX_SNOOZE: u8 = 120;

/// Who sends a preview from the suite itself (a person in the calendar or the control centre).
pub const SUITE: &str = "Torro Workplace";

/// The events the planner needs at `now`: those that may still run, and those whose reminders
/// fire within [`ahead`] (a reminder a day before an event an hour and a half later included).
pub fn window(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    (now - TimeDelta::days(2), now + ahead() + TimeDelta::days(8))
}

/// What the person did with reminders, kept on this computer only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    /// Dismissed reminders, with the end of their event (for tidying up).
    #[serde(default)]
    pub dismissed: BTreeMap<String, DateTime<Utc>>,
    /// Snoozed reminders.
    #[serde(default)]
    pub snoozed: BTreeMap<String, Snoozed>,
    /// Reminders that fired and went to the sink. Taken out again by a snooze, so the reminder
    /// fires once more when the snooze is over.
    #[serde(default)]
    pub shown: BTreeMap<String, Shown>,
    /// Until when all reminders are paused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_until: Option<DateTime<Utc>>,
}

/// A reminder that fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    /// When it went to the sink (the last time, after a snooze).
    pub at: DateTime<Utc>,
    /// When its event ends (for tidying up).
    pub ends: DateTime<Utc>,
}

/// A snoozed reminder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snoozed {
    /// When it fires again.
    pub until: DateTime<Utc>,
    /// When its event ends (for tidying up).
    pub ends: DateTime<Utc>,
}

impl Ledger {
    /// The pause at `now`, if one runs.
    pub fn paused(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.paused_until.filter(|until| *until > now)
    }

    /// Drops what belongs to events that ended [`keep`] ago, and a pause that is over.
    fn tidy(&mut self, now: DateTime<Utc>) {
        let fresh = |ends: &DateTime<Utc>| *ends + keep() > now;
        self.dismissed.retain(|_, ends| fresh(ends));
        self.shown.retain(|_, shown| fresh(&shown.ends));
        self.snoozed.retain(|_, snoozed| fresh(&snoozed.ends));
        if self.paused(now).is_none() {
            self.paused_until = None;
        }
    }
}

/// An instant of an event time: a day counts from local midnight.
fn instant(time: EventTime) -> DateTime<Utc> {
    match time {
        EventTime::DateTime(at) => at,
        EventTime::Date(date) => local_midnight(date),
    }
}

fn local_midnight(date: NaiveDate) -> DateTime<Utc> {
    let midnight = date.and_time(NaiveTime::MIN);
    midnight
        .and_local_timezone(Local)
        .earliest()
        .map(|local| local.with_timezone(&Utc))
        .unwrap_or_else(|| midnight.and_utc())
}

/// When the event starts.
pub fn starts(event: &Event) -> DateTime<Utc> {
    instant(event.start)
}

/// When the event ends for reminders: its end, a day after the start for an all-day entry
/// without one, and never sooner than [`grace`] after the start, so an event without a length
/// still has a moment for its reminder.
pub fn ends(event: &Event) -> DateTime<Utc> {
    let start = starts(event);
    let end = match (event.end, event.start) {
        (Some(end), _) => instant(end),
        (None, EventTime::Date(date)) => local_midnight(date + chrono::Days::new(1)),
        (None, EventTime::DateTime(at)) => at,
    };
    end.max(start + grace())
}

/// The event runs at `now`.
fn running(event: &Event, now: DateTime<Utc>) -> bool {
    starts(event) <= now && now < ends(event)
}

/// The video call of the event: the first call link in its location, else in its notes.
pub fn join_of(event: &Event) -> Option<Join> {
    [event.location.as_deref(), event.description.as_deref()].into_iter().flatten().find_map(|text| {
        torro_gui::links::links(text).into_iter().find_map(|url| {
            torro_gui::links::meeting(url).map(|service| Join { service: service.to_string(), url: url.to_string() })
        })
    })
}

/// The reminders of the event: its own, else one at the default lead.
fn alarms_of(event: &Event, settings: &Settings) -> Vec<Alarm> {
    if !event.alarms.is_empty() {
        return event.alarms.clone();
    }
    settings
        .default_lead
        .map(|minutes| Alarm {
            trigger: AlarmTrigger::Before { minutes: i64::from(minutes), of: AlarmAnchor::Start },
            style: None,
        })
        .into_iter()
        .collect()
}

/// When `trigger` fires for an event from `start` to `end`.
fn fires_at(trigger: AlarmTrigger, start: DateTime<Utc>, end: DateTime<Utc>) -> DateTime<Utc> {
    match trigger {
        AlarmTrigger::At(at) => at,
        AlarmTrigger::Before { minutes, of: AlarmAnchor::Start } => start - TimeDelta::minutes(minutes),
        AlarmTrigger::Before { minutes, of: AlarmAnchor::End } => end - TimeDelta::minutes(minutes),
    }
}

/// The id of the reminder `trigger` of this occurrence of `event`.
pub fn reminder_id(event: &Event, trigger: AlarmTrigger) -> String {
    let trigger = serde_json::to_string(&trigger).unwrap_or_default();
    let start = starts(event).to_rfc3339();
    let mut hash = Sha256::new();
    for part in [event.collection.as_str(), event.uid.as_str(), start.as_str(), trigger.as_str()] {
        hash.update(part.as_bytes());
        hash.update([0u8]);
    }
    hash.finalize().iter().take(16).map(|byte| format!("{byte:02x}")).collect()
}

/// Where the reminder `id`, firing at `fires_at` for an event starting at `start`, stands at
/// `now`; `None` when it is not listed (too far ahead, or missed long ago and never shown).
fn state_of(
    id: &str,
    fires_at: DateTime<Utc>,
    start: DateTime<Utc>,
    ledger: &Ledger,
    now: DateTime<Utc>,
) -> Option<(ReminderState, Option<DateTime<Utc>>)> {
    if ledger.dismissed.contains_key(id) {
        return Some((ReminderState::Dismissed, None));
    }
    if let Some(snoozed) = ledger.snoozed.get(id)
        && snoozed.until > now
    {
        return Some((ReminderState::Snoozed, Some(snoozed.until)));
    }
    if fires_at > now {
        return (fires_at <= now + ahead()).then_some((ReminderState::Upcoming, None));
    }
    let fresh = now <= fires_at.max(start) + grace();
    let known = ledger.shown.contains_key(id) || ledger.snoozed.contains_key(id);
    (fresh || known).then_some((ReminderState::Due, None))
}

/// The style of `alarm` in a calendar shown as `calendar`.
fn style_of(alarm: &Alarm, calendar: CalendarReminders) -> AlarmStyle {
    alarm.style.unwrap_or(match calendar {
        CalendarReminders::FullScreen => AlarmStyle::FullScreen,
        CalendarReminders::Notification | CalendarReminders::Off => AlarmStyle::Notification,
    })
}

/// Another event than `event` with a call link runs at `now`.
fn in_a_call(events: &[Event], event: &Event, now: DateTime<Utc>) -> bool {
    events.iter().any(|other| {
        !(other.uid == event.uid && other.start == event.start) && running(other, now) && join_of(other).is_some()
    })
}

/// The reminders of `events` at `now`, soonest first (see the module).
pub fn plan(events: &[Event], settings: &Settings, ledger: &Ledger, now: DateTime<Utc>) -> Vec<Reminder> {
    if !settings.enabled {
        return Vec::new();
    }
    let mut out = Vec::new();
    for event in events {
        let calendar = settings.calendar(&event.collection);
        let skipped = calendar == CalendarReminders::Off
            || (event.all_day() && !settings.all_day)
            || (event.transparency == Transparency::Transparent && !settings.free_events);
        let (start, end) = (starts(event), ends(event));
        if skipped || now >= end {
            continue;
        }
        let join = join_of(event);
        for alarm in alarms_of(event, settings) {
            let fires_at = fires_at(alarm.trigger, start, end);
            let id = reminder_id(event, alarm.trigger);
            let Some((state, snoozed_until)) = state_of(&id, fires_at, start, ledger, now) else { continue };
            let mut style = style_of(&alarm, calendar);
            if style == AlarmStyle::FullScreen && settings.never_during_call && in_a_call(events, event, now) {
                style = AlarmStyle::Notification;
            }
            out.push(Reminder {
                id,
                event: event.clone(),
                fires_at,
                style,
                state,
                snoozed_until,
                join: join.clone(),
                preview_by: None,
            });
        }
    }
    out.sort_by(|a, b| (a.fires_at, &a.id).cmp(&(b.fires_at, &b.id)));
    out
}

/// A preview on the screen (`calendar.preview_reminder`), in memory only.
#[derive(Debug, Clone)]
struct Preview {
    reminder: Reminder,
    /// Went to the sink already.
    shown: bool,
    /// When it goes by itself.
    until: DateTime<Utc>,
}

/// Where the reminders' commands and the run meet (see the module).
pub struct Reminders {
    file: Option<PathBuf>,
    /// Read on first use.
    ledger: Mutex<Option<Ledger>>,
    previews: Mutex<Vec<Preview>>,
    /// The last preview of each client, by client id.
    last_preview: Mutex<HashMap<String, DateTime<Utc>>>,
    /// Counts every change, so the run plans again at once.
    changed: watch::Sender<u64>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn refusal(code: &str, message: String) -> Response {
    Response::Error { code: code.into(), message }
}

fn unknown(id: &str) -> Response {
    refusal("not-found", format!("no reminder {id:?}"))
}

impl Reminders {
    /// The user's ledger in the core's state folder; tests keep none on disk.
    pub fn system() -> Reminders {
        if cfg!(test) {
            return Reminders::new(None);
        }
        Reminders::new(AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join(FILE)))
    }

    /// Reminders kept in `file`, or only in memory without one.
    pub fn new(file: Option<PathBuf>) -> Reminders {
        Reminders {
            file,
            ledger: Mutex::new(None),
            previews: Mutex::new(Vec::new()),
            last_preview: Mutex::new(HashMap::new()),
            changed: watch::channel(0).0,
        }
    }

    /// Changes after each command, for the run.
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// Runs `change` on the ledger (read on first use) and writes it when `change` says so.
    fn with_ledger<T>(&self, now: DateTime<Utc>, change: impl FnOnce(&mut Ledger) -> (T, bool)) -> T {
        let mut slot = lock(&self.ledger);
        let ledger = slot.get_or_insert_with(|| self.file.as_deref().map(read).unwrap_or_default());
        let (answer, changed) = change(ledger);
        if changed {
            ledger.tidy(now);
            if let Some(file) = &self.file
                && let Err(err) = write(file, ledger)
            {
                eprintln!("torroworkplace: cannot save the reminders: {err}");
            }
            self.changed.send_modify(|count| *count = count.wrapping_add(1));
        }
        answer
    }

    /// The ledger as it is now.
    pub fn ledger(&self, now: DateTime<Utc>) -> Ledger {
        self.with_ledger(now, |ledger| (ledger.clone(), false))
    }

    /// The reminders of `events` and the previews, and the pause.
    pub fn reminders(
        &self,
        events: &[Event],
        settings: &Settings,
        now: DateTime<Utc>,
    ) -> (Vec<Reminder>, Option<DateTime<Utc>>) {
        let ledger = self.ledger(now);
        let mut all = plan(events, settings, &ledger, now);
        {
            let mut previews = lock(&self.previews);
            previews.retain(|preview| preview.until > now);
            all.splice(0..0, previews.iter().map(|preview| preview.reminder.clone()));
        }
        (all, ledger.paused(now))
    }

    /// `calendar.list_reminders` on `events`, the events the client may read.
    pub fn list(&self, events: &[Event], settings: &Settings, now: DateTime<Utc>, due_only: bool) -> Response {
        let (mut reminders, paused_until) = self.reminders(events, settings, now);
        if due_only {
            reminders.retain(|reminder| reminder.state == ReminderState::Due);
        }
        Response::Reminders { reminders, paused_until }
    }

    /// The reminder `id` among those of `events` that fired: `Err` with the answer otherwise.
    fn fired(
        &self,
        id: &str,
        events: &[Event],
        settings: &Settings,
        now: DateTime<Utc>,
    ) -> Result<Reminder, Box<Response>> {
        let (reminders, _) = self.reminders(events, settings, now);
        let reminder = reminders.into_iter().find(|reminder| reminder.id == id).ok_or_else(|| Box::new(unknown(id)))?;
        match reminder.state {
            ReminderState::Due | ReminderState::Snoozed | ReminderState::Dismissed => Ok(reminder),
            ReminderState::Upcoming => Err(Box::new(refusal(
                "conflict",
                "this reminder has not fired yet; reminders are snoozed or dismissed once they come".into(),
            ))),
        }
    }

    /// Takes the preview `id` off the screen; `false` when there is none.
    fn drop_preview(&self, id: &str) -> bool {
        let mut previews = lock(&self.previews);
        let before = previews.len();
        previews.retain(|preview| preview.reminder.id != id);
        let dropped = previews.len() != before;
        if dropped {
            self.changed.send_modify(|count| *count = count.wrapping_add(1));
        }
        dropped
    }

    /// `calendar.snooze_reminder`.
    pub fn snooze(
        &self,
        id: &str,
        until: Snooze,
        events: &[Event],
        settings: &Settings,
        now: DateTime<Utc>,
    ) -> Response {
        if let Snooze::Minutes(minutes) = until
            && !(1..=MAX_SNOOZE).contains(&minutes)
        {
            return refusal("error", format!("snooze for 1 to {MAX_SNOOZE} minutes, not {minutes}"));
        }
        // A preview has nothing to come back for: snoozing it ends it.
        if self.drop_preview(id) {
            return Response::Ok;
        }
        let reminder = match self.fired(id, events, settings, now) {
            Ok(reminder) => reminder,
            Err(answer) => return *answer,
        };
        if reminder.state == ReminderState::Dismissed {
            return refusal("conflict", "this reminder was dismissed".into());
        }
        let start = starts(&reminder.event);
        let until = match until {
            Snooze::Minutes(minutes) => now + TimeDelta::minutes(i64::from(minutes)),
            Snooze::UntilStart if start > now => start,
            Snooze::UntilStart => return refusal("unsupported", "the event has started already".into()),
        };
        let ends = ends(&reminder.event);
        self.with_ledger(now, |ledger| {
            ledger.snoozed.insert(id.to_string(), Snoozed { until, ends });
            ledger.shown.remove(id);
            (Response::Ok, true)
        })
    }

    /// `calendar.dismiss_reminder`.
    pub fn dismiss(&self, id: &str, events: &[Event], settings: &Settings, now: DateTime<Utc>) -> Response {
        if self.drop_preview(id) {
            return Response::Ok;
        }
        let reminder = match self.fired(id, events, settings, now) {
            Ok(reminder) => reminder,
            Err(answer) => return *answer,
        };
        let ends = ends(&reminder.event);
        self.with_ledger(now, |ledger| {
            let changed = ledger.dismissed.insert(id.to_string(), ends).is_none();
            ledger.snoozed.remove(id);
            (Response::Ok, changed)
        })
    }

    /// `calendar.pause_reminders`; whether the caller may pause is the core's check.
    pub fn pause(&self, until: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Response {
        if let Some(until) = until
            && until > now + max_pause()
        {
            return refusal("error", "reminders pause for at most 24 hours".into());
        }
        let until = until.filter(|until| *until > now);
        self.with_ledger(now, |ledger| {
            let changed = ledger.paused_until != until;
            ledger.paused_until = until;
            (Response::Ok, changed)
        })
    }

    /// `calendar.preview_reminder` for `event`: shown now, in the style of its first reminder or
    /// its calendar, as sent `by`. A client (`client`, its id) may ask once a minute, and its
    /// preview keeps the pause and calls in mind: while paused it is `busy`, during a call it
    /// comes as a notification.
    pub fn preview(
        &self,
        event: &Event,
        settings: &Settings,
        events: &[Event],
        by: &str,
        client: Option<&str>,
        now: DateTime<Utc>,
    ) -> Response {
        if let Some(client) = client {
            if self.ledger(now).paused(now).is_some() {
                return refusal("busy", "reminders are paused; a preview would not show either".into());
            }
            let mut last = lock(&self.last_preview);
            if last.get(client).is_some_and(|at| now < *at + preview_every()) {
                return refusal("busy", "one preview a minute; try again shortly".into());
            }
            last.insert(client.to_string(), now);
        }
        let alarm = alarms_of(event, settings)
            .into_iter()
            .next()
            .unwrap_or(Alarm { trigger: AlarmTrigger::Before { minutes: 0, of: AlarmAnchor::Start }, style: None });
        let mut style = style_of(&alarm, settings.calendar(&event.collection));
        if client.is_some() && settings.never_during_call && in_a_call(events, event, now) {
            style = AlarmStyle::Notification;
        }
        let id = format!("preview-{}", &reminder_id(event, AlarmTrigger::At(now))[..24]);
        let reminder = Reminder {
            id,
            event: event.clone(),
            fires_at: now,
            style,
            state: ReminderState::Due,
            snoozed_until: None,
            join: join_of(event),
            preview_by: Some(by.to_string()),
        };
        lock(&self.previews).push(Preview { reminder, shown: false, until: now + preview_for() });
        self.changed.send_modify(|count| *count = count.wrapping_add(1));
        Response::Ok
    }

    /// The reminders that became due since the last call (previews included), marked as shown, and
    /// whether reminders are paused. For the run.
    pub fn take_due(&self, events: &[Event], settings: &Settings, now: DateTime<Utc>) -> (Vec<Reminder>, bool) {
        let ledger = self.ledger(now);
        let fresh: Vec<Reminder> = plan(events, settings, &ledger, now)
            .into_iter()
            .filter(|reminder| reminder.state == ReminderState::Due && !ledger.shown.contains_key(&reminder.id))
            .collect();
        if !fresh.is_empty() {
            self.with_ledger(now, |ledger| {
                for reminder in &fresh {
                    ledger.shown.insert(reminder.id.clone(), Shown { at: now, ends: ends(&reminder.event) });
                    ledger.snoozed.remove(&reminder.id);
                }
                ((), true)
            });
        }
        let mut due = Vec::new();
        for preview in lock(&self.previews).iter_mut().filter(|preview| !preview.shown && preview.until > now) {
            preview.shown = true;
            due.push(preview.reminder.clone());
        }
        due.extend(fresh);
        (due, ledger.paused(now).is_some())
    }

    /// The reminders that fired and stand now, with when they fired: for their inbox entries (task
    /// ER1.4). Previews have none.
    pub fn standing(
        &self,
        events: &[Event],
        settings: &Settings,
        now: DateTime<Utc>,
    ) -> Vec<(Reminder, DateTime<Utc>)> {
        let ledger = self.ledger(now);
        plan(events, settings, &ledger, now)
            .into_iter()
            .filter(|reminder| reminder.state == ReminderState::Due)
            .filter_map(|reminder| {
                let fired = ledger.shown.get(&reminder.id)?.at;
                Some((reminder, fired))
            })
            .collect()
    }

    /// Makes the reminders follow what the person did with their inbox entries (task ER1.4): an
    /// entry done after its reminder fired dismisses it, a snoozed one snoozes it until then (or
    /// dismisses it when that is after its event). Only reminders that stand are touched, so the
    /// entries the inbox closes itself change nothing.
    pub fn follow_inbox(
        &self,
        answers: &[(String, crate::inbox::ReminderAnswer)],
        events: &[Event],
        settings: &Settings,
        now: DateTime<Utc>,
    ) {
        let standing = self.standing(events, settings, now);
        let followed: Vec<(String, Option<Snoozed>, DateTime<Utc>)> = answers
            .iter()
            .filter_map(|(id, answer)| {
                let (reminder, fired) = standing.iter().find(|(reminder, _)| reminder.id == *id)?;
                let ends = ends(&reminder.event);
                match *answer {
                    crate::inbox::ReminderAnswer::Done { at } if at >= *fired => Some((id.clone(), None, ends)),
                    crate::inbox::ReminderAnswer::Done { .. } => None,
                    crate::inbox::ReminderAnswer::Snoozed { until } if until < ends => {
                        Some((id.clone(), Some(Snoozed { until, ends }), ends))
                    }
                    crate::inbox::ReminderAnswer::Snoozed { .. } => Some((id.clone(), None, ends)),
                }
            })
            .collect();
        if followed.is_empty() {
            return;
        }
        self.with_ledger(now, |ledger| {
            for (id, snoozed, ends) in followed {
                ledger.shown.remove(&id);
                match snoozed {
                    Some(snoozed) => {
                        ledger.snoozed.insert(id, snoozed);
                    }
                    None => {
                        ledger.dismissed.insert(id, ends);
                    }
                }
            }
            ((), true)
        });
    }

    /// When the run should look again: the next reminder that fires or the next snooze that ends,
    /// within a minute.
    pub fn next_wake(&self, events: &[Event], settings: &Settings, now: DateTime<Utc>) -> DateTime<Utc> {
        let minute = now + TimeDelta::minutes(1);
        let (reminders, _) = self.reminders(events, settings, now);
        reminders
            .iter()
            .filter_map(|reminder| match reminder.state {
                ReminderState::Upcoming => Some(reminder.fires_at),
                ReminderState::Snoozed => reminder.snoozed_until,
                ReminderState::Due | ReminderState::Dismissed => None,
            })
            .filter(|at| *at > now)
            .min()
            .map_or(minute, |at| at.min(minute))
    }
}

/// The ledger in `file`; a missing or broken file is an empty ledger (a broken one is logged).
fn read(file: &Path) -> Ledger {
    match std::fs::read(file) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            eprintln!("torroworkplace: {} does not read, starting a new one: {err}", file.display());
            Ledger::default()
        }),
        Err(_) => Ledger::default(),
    }
}

/// Writes `ledger` to `file` (0600), creating the folder.
fn write(file: &Path, ledger: &Ledger) -> Result<(), String> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|err| format!("{}: {err}", folder.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(ledger).map_err(|err| err.to_string())?;
    crate::clients::write_private(file, &bytes).map_err(|err| err.to_string())
}

/// Where due reminders show: a notification or the full screen (tasks ER1.4, ER2.1). Their inbox
/// entries are the run's own.
pub trait Sink: Send + Sync {
    /// `due` became due now; `paused` while reminders are paused (then they show nowhere, their
    /// inbox entries stay). `settings` say the sound.
    fn fire(&self, due: &[Reminder], paused: bool, settings: &Settings);
}

/// The desktop's sink (tasks ER1.4, ER2.1, ER2.2): a reminder in the style "notification" becomes a
/// notification of the system with Join (when there is a call), In 5 min and Dismiss, sent back to
/// `core` as the suite itself; one in the style "full screen" starts the reminder window
/// (`torrocalendar --reminders`, on every screen with the setting `allDisplays`), once while it
/// runs, and gets no notification besides. On Hyprland the window manager learns the window's
/// place first, and the windows follow a change of workspace. Should the window not start, its
/// reminders come as notifications. Quiet during busy events holds nothing back: a reminder is what
/// the person waits for during a meeting.
pub struct Desktop {
    core: std::sync::Weak<crate::core::Core>,
    /// The reminder window while it runs, shared with the task that starts it.
    window: Arc<Mutex<Option<std::process::Child>>>,
}

impl Desktop {
    pub fn new(core: &Arc<crate::core::Core>) -> Desktop {
        Desktop { core: Arc::downgrade(core), window: Arc::new(Mutex::new(None)) }
    }
}

/// Which of `due` show as notifications, and whether the full screen is wanted for the others.
pub fn route(due: &[Reminder]) -> (Vec<&Reminder>, bool) {
    let (full, notify): (Vec<&Reminder>, Vec<&Reminder>) =
        due.iter().partition(|reminder| reminder.style == AlarmStyle::FullScreen);
    (notify, !full.is_empty())
}

/// The arguments of the reminder window: every screen or the main one, and the monitors the window
/// manager prepared (main first).
pub fn window_arguments(all_displays: bool, monitors: &[String]) -> Vec<String> {
    let mut arguments = vec!["--reminders".to_string()];
    if all_displays {
        arguments.push("--all-displays".into());
    }
    if !monitors.is_empty() {
        arguments.push("--monitors".into());
        arguments.push(monitors.join(","));
    }
    arguments
}

/// Whether the window in `slot` runs.
fn runs(slot: &Mutex<Option<std::process::Child>>) -> bool {
    lock(slot).as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(None)))
}

/// The program of the reminder window: on macOS the agent app "Torro Reminder.app" (task ER2.2),
/// beside the core's own bundle or in an Applications folder, so it shows over apps in full screen;
/// else, and without that bundle, the calendar next to the core (`torrocalendar --reminders`).
pub fn reminder_program(core: Option<&Path>, home: Option<&Path>, exists: impl Fn(&Path) -> bool) -> PathBuf {
    if cfg!(target_os = "macos") {
        let inside = Path::new("Torro Reminder.app/Contents/MacOS/torro-reminder");
        // The core runs as …/Torro Workplace.app/Contents/MacOS/torroworkplace.
        let beside = core.and_then(|exe| exe.ancestors().nth(4)).map(|folder| folder.join(inside));
        let applications = [Some(PathBuf::from("/Applications")), home.map(|home| home.join("Applications"))];
        let candidates = beside.into_iter().chain(applications.into_iter().flatten().map(|folder| folder.join(inside)));
        if let Some(found) = candidates.into_iter().find(|candidate| exists(candidate)) {
            return found;
        }
    }
    crate::core::sibling(AppId::CALENDAR.binary)
}

/// Starts the reminder window with `arguments` into `slot`; `false` when it cannot be started.
fn start_window(slot: &Mutex<Option<std::process::Child>>, arguments: &[String]) -> bool {
    let core = std::env::current_exe().ok();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let program = reminder_program(core.as_deref(), home.as_deref(), Path::is_file);
    let mut window = lock(slot);
    match std::process::Command::new(&program).args(arguments).spawn() {
        Ok(child) => {
            *window = Some(child);
            true
        }
        Err(err) => {
            eprintln!("torroworkplace: cannot start the reminder window {}: {err}", program.display());
            *window = None;
            false
        }
    }
}

impl Sink for Desktop {
    fn fire(&self, due: &[Reminder], paused: bool, settings: &Settings) {
        if paused {
            return;
        }
        let sound = settings.sound == torro_core::settings::ReminderSound::System;
        let (notify, full) = route(due);
        for reminder in notify {
            show_notification(self.core.clone(), reminder, sound);
        }
        // A running window reads the new reminder itself.
        if !full || runs(&self.window) {
            return;
        }
        let Some(core) = self.core.upgrade() else { return };
        let full: Vec<Reminder> =
            due.iter().filter(|reminder| reminder.style == AlarmStyle::FullScreen).cloned().collect();
        let (slot, all_displays, weak) = (Arc::clone(&self.window), settings.all_displays, self.core.clone());
        tokio::spawn(async move {
            // The window manager learns the place first, so the window never shows elsewhere.
            let monitors = core.cover_monitors_for_reminders(all_displays).await;
            if !start_window(&slot, &window_arguments(all_displays, &monitors)) {
                for reminder in &full {
                    show_notification(weak.clone(), reminder, sound);
                }
                return;
            }
            if !monitors.is_empty() {
                core.keep_reminder_windows_in_view().await;
            }
        });
    }
}

/// Shows the notification of `reminder`; its buttons act through `core`.
fn show_notification(core: std::sync::Weak<crate::core::Core>, reminder: &Reminder, sound: bool) {
    let (id, join) = (reminder.id.clone(), reminder.join.clone());
    crate::inbox_notify::show_reminder(
        reminder,
        sound,
        Box::new(move |answer| {
            let Some(core) = core.upgrade() else { return };
            answer_notification(core, id, join, answer);
        }),
    );
}

/// What a button of a reminder's notification does: Join opens the call, In 5 min and Dismiss send
/// their command as the suite, a click opens the calendar.
fn answer_notification(
    core: Arc<crate::core::Core>,
    id: String,
    join: Option<Join>,
    answer: crate::inbox_notify::ReminderAnswer,
) {
    let Some(request) = notification_request(id, answer) else {
        if let Some(join) = join
            && let Err(err) = core.open_call(&join.url)
        {
            eprintln!("torroworkplace: cannot open the call of a reminder: {err}");
        }
        return;
    };
    tokio::spawn(async move {
        if let Response::Error { code, message } = core.handle(request).await {
            eprintln!("torroworkplace: a reminder's button failed ({code}): {message}");
        }
    });
}

/// The command a button of the notification of the reminder `id` sends; `None` for Join, which
/// opens the call in the browser instead.
pub(crate) fn notification_request(
    id: String,
    answer: crate::inbox_notify::ReminderAnswer,
) -> Option<torro_ipc::Request> {
    use crate::inbox_notify::ReminderAnswer;
    Some(match answer {
        ReminderAnswer::Join => return None,
        ReminderAnswer::Snooze => torro_ipc::Request::SnoozeReminder { id, until: Snooze::Minutes(5) },
        ReminderAnswer::Dismiss => torro_ipc::Request::DismissReminder { id },
        ReminderAnswer::Open => torro_ipc::Request::OpenApp { app: AppId::CALENDAR.binary.to_string(), target: None },
    })
}

/// Plans and fires the reminders of `core` until the core goes away (see the module).
pub async fn run(core: Arc<crate::core::Core>, sink: Arc<dyn Sink>) {
    let mut changes = core.reminders().changes();
    loop {
        let now = Utc::now();
        let (events, settings) = core.reminder_inputs(now).await;
        // What the person did in the inbox first, so a reminder put away there shows no more.
        match core.inbox().reminder_answers() {
            Ok(answers) => core.reminders().follow_inbox(&answers, &events, &settings, now),
            Err(err) => eprintln!("torroworkplace: cannot read the reminders' inbox entries: {err}"),
        }
        let (due, paused) = core.reminders().take_due(&events, &settings, now);
        if !due.is_empty() {
            sink.fire(&due, paused, &settings);
        }
        let standing = core.reminders().standing(&events, &settings, now);
        if let Err(err) = core.inbox().file_reminders(&standing, now) {
            eprintln!("torroworkplace: cannot file the reminders in the inbox: {err}");
        }
        let wake = core.reminders().next_wake(&events, &settings, now);
        let wait = (wake - now).to_std().unwrap_or_default().clamp(std::time::Duration::from_secs(1), MINUTE);
        tokio::select! {
            changed = changes.changed() => {
                if changed.is_err() {
                    return;
                }
                changes.borrow_and_update();
            }
            () = tokio::time::sleep(wait) => {}
        }
    }
}

/// The longest the run sleeps.
const MINUTE: std::time::Duration = std::time::Duration::from_secs(60);

#[cfg(test)]
mod tests;
