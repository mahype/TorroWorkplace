//! Changing and deleting events of a series (task TE.4, `docs/termin-editor.md` decisions 3 to 6).
//!
//! Pure functions on the iCalendar object the server stores for a series (its main event with the
//! rule, its changed occurrences and its zones), for one occurrence the client names by its key
//! (`Event::recurrence_id`) and a [`SeriesScope`]:
//!
//! - **Occurrence:** changing it stores a changed occurrence (a `VEVENT` with the series' UID and
//!   `RECURRENCE-ID`, made from the main event without its rule); deleting it adds an `EXDATE` (in the
//!   zone of `DTSTART`) and drops a changed occurrence.
//! - **Series:** the main event changes as the occurrence changed: the fields the client changed, and a
//!   start moved by the same distance on the clock of the series. The keys of the changed occurrences
//!   and every `EXDATE` move along; what a changed occurrence changed itself stays. A new rule drops
//!   the changed occurrences it no longer has; no rule makes the series a single event (the
//!   occurrence the client changed). Deleting deletes the object.
//! - **Following:** splits the series as Nextcloud and Google do (never `RANGE=THISANDFUTURE`): the
//!   stored series ends the second before the occurrence (a `COUNT` becomes `UNTIL`), a new object with
//!   a new UID and `RELATED-TO;RELTYPE=SIBLING` starts at it with the rest of the count, and the later
//!   changed occurrences and `EXDATE`s move into it – unless time or rule change, then they are reset
//!   and [`Outcome::reset`] says how many. The first occurrence counts as the whole series. Deleting
//!   only ends the stored series.
//!
//! Whatever the suite does not know stays as stored: properties of other programs, alarms, zones. The
//! server keeps expanding the occurrences (decision 1); the `rrule` crate only counts dates here: the
//! rest of a `COUNT`, whether a key belongs to the series, and which events a change drops
//! ([`drops`]), which the core needs to decide whether the save needs the right to delete.

use std::fmt;

use chrono::{DateTime, Duration, NaiveDateTime, NaiveTime, Utc};
use chrono_tz::Tz;
use torro_pim::model::{Repeat, RepeatEnd, SeriesScope};
use torro_pim::{Event, EventTime, ProviderError};

use crate::ical::{self, Property};
use crate::write;

/// At most this many dates are counted per rule; a rule that goes on is compared only as far.
const LIMIT: u16 = 5000;
/// Dates are counted at most this many days after the series starts.
const HORIZON_DAYS: i64 = 3660;

/// The properties that make a main event a series; a changed occurrence has none of them.
const RULE_PROPERTIES: [&str; 4] = ["RRULE", "RDATE", "EXDATE", "EXRULE"];

/// What a write of an occurrence does to the stored object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// The stored object becomes `ics` (written with `If-Match`).
    Replace { ics: String },
    /// "This and all following": `new` is a new object (UID [`Writing::new_uid`], written with
    /// `If-None-Match: *` first) holding the occurrence and those after it; the stored object then
    /// becomes `old`, which ends the second before it.
    Split { new: String, old: String },
    /// Nothing of the series is left: the stored object is deleted (with `If-Match`).
    Delete,
}

/// What [`save`] or [`delete`] plans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub plan: Plan,
    /// Changed occurrences that fall away beyond the one the client changed: those a new rule does
    /// not have any more, those after a split whose time or rule changed, all of them when the series
    /// becomes a single event. Always 0 for a delete, which says what it removes.
    pub reset: u32,
    /// The key (`RECURRENCE-ID`) the occurrence the client changed has after the write, in the new
    /// object of a [`Plan::Split`]; `None` for a single event and after a delete.
    pub key: Option<EventTime>,
}

/// When and how a change is written.
#[derive(Debug, Clone, Copy)]
pub struct Writing<'a> {
    /// For `DTSTAMP` and `LAST-MODIFIED`.
    pub now: DateTime<Utc>,
    /// The computer's zone ([`crate::write::machine_zone`]), for times of an event that names none.
    pub local: Option<Tz>,
    /// The UID of the new object a split makes; the caller makes it ([`crate::write`] has one).
    pub new_uid: &'a str,
}

/// Why a change of a series is refused before anything is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeriesError {
    /// The event names no occurrence of the series stored now: `recurrence_id` is missing, or the
    /// series has no such date (any more). The client reloads the events.
    NoOccurrence,
    /// A change the suite cannot write yet, with the reason for people.
    Unsupported(&'static str),
}

impl SeriesError {
    /// The API's error code: `not-found` or `unsupported`.
    pub fn code(&self) -> &'static str {
        match self {
            SeriesError::NoOccurrence => "not-found",
            SeriesError::Unsupported(_) => "unsupported",
        }
    }
}

impl fmt::Display for SeriesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SeriesError::NoOccurrence => f.write_str(
                "the series has no such occurrence (any more): reload the events and send its recurrence_id",
            ),
            SeriesError::Unsupported(why) => f.write_str(why),
        }
    }
}

/// Why a write to a series failed: the server's answer, or the series refused the change.
#[derive(Debug, Clone, PartialEq)]
pub enum SeriesFailure {
    Server(ProviderError),
    Refused(SeriesError),
}

impl From<ProviderError> for SeriesFailure {
    fn from(err: ProviderError) -> SeriesFailure {
        SeriesFailure::Server(err)
    }
}

impl From<SeriesError> for SeriesFailure {
    fn from(err: SeriesError) -> SeriesFailure {
        SeriesFailure::Refused(err)
    }
}

const KIND_CHANGE: &str =
    "a series cannot change between all-day and timed events yet: change one event, or make a new series";
const NO_MAIN: &str =
    "the server holds only changed events of this series, not the series itself: change them one by one";

/// What changing the occurrence `event` of the series `stored` with `scope` writes (see the module
/// documentation). `event` is the occurrence as the client wants it, with the `recurrence_id` it was
/// read with; a stored object that is no series is written as [`crate::write::merge_ics_event`] does,
/// a new rule included, whatever the scope.
pub fn save(stored: &str, event: &Event, scope: SeriesScope, writing: &Writing<'_>) -> Result<Outcome, SeriesError> {
    let found = find(stored, event)?;
    let Some(key) = found.key else {
        let ics = write::event_onto(stored, event, writing.now, writing.local);
        return Ok(Outcome { plan: Plan::Replace { ics }, reset: 0, key: None });
    };
    match scope {
        SeriesScope::Occurrence => save_occurrence(found, key, event, writing),
        SeriesScope::Following if !found.is_first(key) => save_following(found, key, event, writing),
        SeriesScope::Following | SeriesScope::Series => save_series(found, key, event, writing),
    }
}

/// What deleting the occurrence `event` of the series `stored` with `scope` writes: an `EXDATE`
/// (occurrence), the end of the series before it (following; the whole object from the first one) or
/// nothing at all (series, and a stored object that is no series).
pub fn delete(stored: &str, event: &Event, scope: SeriesScope, now: DateTime<Utc>) -> Result<Outcome, SeriesError> {
    let found = find(stored, event)?;
    let gone = Outcome { plan: Plan::Delete, reset: 0, key: None };
    let Some(key) = found.key else { return Ok(gone) };
    let mut object = found.object;
    match (scope, &found.main) {
        (SeriesScope::Series, _) => Ok(gone),
        (SeriesScope::Following, Some(main)) if main.event.start == key => Ok(gone),
        (SeriesScope::Following, Some(main)) => {
            let head = head(&object, main, key, now);
            Ok(Outcome { plan: Plan::Replace { ics: head.text() }, reset: 0, key: None })
        }
        (SeriesScope::Following, None) => {
            object.retain_vevents(|block| key_of(block).is_none_or(|k| instant(k) < instant(key)));
            Ok(if object.vevents().next().is_none() { gone } else { replaced(&object) })
        }
        (SeriesScope::Occurrence, main) => {
            object.retain_vevents(|block| key_of(block) != Some(key));
            if let Some(main) = main {
                let excluded = write::time_line("EXDATE", key, main.zone);
                object.edit_master(|block| touch(&insert(block, vec![excluded]), now));
            }
            Ok(if object.vevents().next().is_none() { gone } else { replaced(&object) })
        }
    }
}

/// How many events of the series `stored` a save of `event` with `scope` removes or resets: dates the
/// series no longer has (a shorter `UNTIL` or `COUNT`, another rule; compared as far as dates are
/// counted) and changed occurrences that fall away ([`resets`]). At least 1 when the rule goes. Read
/// from the stored object only, never from what the client says the series is; when it cannot tell
/// (no such occurrence, a rule the counter cannot read and a change of time or rule), 1. Changing a
/// single occurrence and a stored single event drop nothing.
pub fn drops(stored: &str, event: &Event, scope: SeriesScope) -> u32 {
    lost(stored, event, scope).map_or(1, |lost| lost.dropped)
}

/// How many changed occurrences of the series `stored`, beyond the one `event` is, a save with
/// `scope` resets ([`Outcome::reset`]); 0 when it cannot tell.
pub fn resets(stored: &str, event: &Event, scope: SeriesScope) -> u32 {
    lost(stored, event, scope).map_or(0, |lost| lost.reset)
}

// ---------------------------------------------------------------------------------------------
// The stored object

/// One part of a stored object: a line outside the `VEVENT`s, or a whole `VEVENT` (unfolded lines
/// from its `BEGIN` to its `END`).
#[derive(Debug, Clone)]
enum Part {
    Line(String),
    Vevent(Vec<String>),
}

/// A stored iCalendar object, unfolded, with its `VEVENT`s as blocks so they can be changed, dropped
/// or added while everything else keeps its place.
#[derive(Debug, Clone)]
struct Object {
    parts: Vec<Part>,
}

impl Object {
    fn read(ics: &str) -> Object {
        let mut parts = Vec::new();
        let mut block: Option<Vec<String>> = None;
        // 1 inside the VCALENDAR, 2 inside one of its components, more in nested ones.
        let mut depth = 0usize;
        for line in ical::unfold(ics) {
            let (begin, end, vevent) = match ical::parse_line(&line) {
                Some(prop) => (
                    prop.name.eq_ignore_ascii_case("BEGIN"),
                    prop.name.eq_ignore_ascii_case("END"),
                    prop.value.trim().eq_ignore_ascii_case("VEVENT"),
                ),
                None => (false, false, false),
            };
            if begin && depth == 1 && vevent {
                block = Some(Vec::new());
            }
            if begin {
                depth += 1;
            }
            if end {
                depth = depth.saturating_sub(1);
            }
            match block.as_mut() {
                Some(lines) => lines.push(line),
                None => {
                    parts.push(Part::Line(line));
                    continue;
                }
            }
            if end
                && depth == 1
                && let Some(done) = block.take()
            {
                parts.push(Part::Vevent(done));
            }
        }
        Object { parts }
    }

    /// The object as text: folded at 75 octets, CRLF line ends.
    fn text(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Line(line) => write::push_line(&mut out, line),
                Part::Vevent(lines) => lines.iter().for_each(|line| write::push_line(&mut out, line)),
            }
        }
        out
    }

    fn vevents(&self) -> impl Iterator<Item = &Vec<String>> {
        self.parts.iter().filter_map(|part| match part {
            Part::Vevent(block) => Some(block),
            Part::Line(_) => None,
        })
    }

    fn vevents_mut(&mut self) -> impl Iterator<Item = &mut Vec<String>> {
        self.parts.iter_mut().filter_map(|part| match part {
            Part::Vevent(block) => Some(block),
            Part::Line(_) => None,
        })
    }

    /// The main event: the first `VEVENT` without `RECURRENCE-ID`.
    fn master(&self) -> Option<&Vec<String>> {
        self.vevents().find(|block| is_master(block))
    }

    /// Replaces the main event's block with what `change` makes of it.
    fn edit_master(&mut self, change: impl FnOnce(&[String]) -> Vec<String>) {
        if let Some(block) = self.vevents_mut().find(|block| is_master(block)) {
            *block = change(block);
        }
    }

    /// The changed occurrence with the key `key`.
    fn exception(&self, key: EventTime) -> Option<&Vec<String>> {
        self.vevents().find(|block| key_of(block) == Some(key))
    }

    /// The keys of the changed occurrences, in the stored order.
    fn exception_keys(&self) -> Vec<EventTime> {
        self.vevents().filter_map(|block| key_of(block)).collect()
    }

    fn retain_vevents(&mut self, mut keep: impl FnMut(&[String]) -> bool) {
        self.parts.retain(|part| match part {
            Part::Vevent(block) => keep(block),
            Part::Line(_) => true,
        });
    }

    /// Puts `block` in place of the changed occurrence with its key, or after the last `VEVENT`.
    fn put_exception(&mut self, key: EventTime, block: Vec<String>) {
        if let Some(stored) = self.vevents_mut().find(|stored| key_of(stored) == Some(key)) {
            *stored = block;
            return;
        }
        let last = self.parts.iter().rposition(|part| matches!(part, Part::Vevent(_)));
        let at = last.map_or_else(|| self.parts.len().saturating_sub(1), |last| last + 1);
        self.parts.insert(at, Part::Vevent(block));
    }

    /// The `TZID`s of the object's `VTIMEZONE`s.
    fn zones(&self) -> Vec<String> {
        write::defined_zones(&self.text())
    }

    /// Adds `zone` (one unfolded `VTIMEZONE`, possibly empty) before the first `VEVENT`.
    fn add_zone(&mut self, zone: Vec<String>) {
        if zone.is_empty() {
            return;
        }
        let at = self.parts.iter().position(|part| matches!(part, Part::Vevent(_))).unwrap_or(self.parts.len());
        self.parts.splice(at..at, zone.into_iter().map(Part::Line));
    }
}

/// `object` as the stored object becomes.
fn replaced(object: &Object) -> Outcome {
    Outcome { plan: Plan::Replace { ics: object.text() }, reset: 0, key: None }
}

fn is_master(block: &[String]) -> bool {
    first(block, "RECURRENCE-ID").is_none()
}

/// The own properties of a `VEVENT` block with their line numbers: those outside the components
/// nested in it (`VALARM`).
fn own(block: &[String]) -> Vec<(usize, Property<'_>)> {
    let mut depth = 0usize;
    let mut found = Vec::new();
    for (i, line) in block.iter().enumerate() {
        let Some(prop) = ical::parse_line(line) else { continue };
        if prop.name.eq_ignore_ascii_case("BEGIN") {
            depth += 1;
        } else if prop.name.eq_ignore_ascii_case("END") {
            depth = depth.saturating_sub(1);
        } else if depth == 1 {
            found.push((i, prop));
        }
    }
    found
}

fn first<'a>(block: &'a [String], name: &str) -> Option<Property<'a>> {
    own(block).into_iter().map(|(_, prop)| prop).find(|prop| prop.name.eq_ignore_ascii_case(name))
}

fn all<'a>(block: &'a [String], name: &str) -> Vec<Property<'a>> {
    own(block).into_iter().map(|(_, prop)| prop).filter(|prop| prop.name.eq_ignore_ascii_case(name)).collect()
}

/// `block` without its own properties named in `names`, with `lines` right after its `BEGIN`.
fn set(block: &[String], names: &[&str], lines: Vec<String>) -> Vec<String> {
    let dropped: Vec<usize> = own(block)
        .into_iter()
        .filter(|(_, prop)| names.iter().any(|name| prop.name.eq_ignore_ascii_case(name)))
        .map(|(i, _)| i)
        .collect();
    let mut out = Vec::with_capacity(block.len() + lines.len());
    let mut lines = Some(lines);
    for (i, line) in block.iter().enumerate() {
        if dropped.contains(&i) {
            continue;
        }
        out.push(line.clone());
        if let Some(lines) = lines.take() {
            out.extend(lines);
        }
    }
    out
}

/// `block` with `lines` added right after its `BEGIN`.
fn insert(block: &[String], lines: Vec<String>) -> Vec<String> {
    set(block, &[], lines)
}

/// `block` with a new `DTSTAMP` and `LAST-MODIFIED`, for a change made line by line.
fn touch(block: &[String], now: DateTime<Utc>) -> Vec<String> {
    let stamp = ical::format_utc(now);
    set(block, &["DTSTAMP", "LAST-MODIFIED"], vec![format!("DTSTAMP:{stamp}"), format!("LAST-MODIFIED:{stamp}")])
}

/// The time a property like `DTSTART` holds in `value`, read as the reader reads `DTSTART`: a day, a
/// UTC time, a time in its `TZID`, else in the computer's zone.
fn time_of(prop: &Property<'_>, value: &str) -> Option<EventTime> {
    let start = Property { name: "DTSTART", params: prop.params.clone(), value };
    ical::to_event(std::slice::from_ref(&start), &[], "").map(|event| event.start)
}

/// The times of a list property (`EXDATE`, `RDATE`); values that are no time (`PERIOD`) are left out.
fn times_of(prop: &Property<'_>) -> Vec<EventTime> {
    prop.value.split(',').filter_map(|value| time_of(prop, value.trim())).collect()
}

/// The key of a changed occurrence (`RECURRENCE-ID`).
fn key_of(block: &[String]) -> Option<EventTime> {
    first(block, "RECURRENCE-ID").and_then(|prop| time_of(&prop, prop.value))
}

/// The event a `VEVENT` block describes, as the reader reads it.
fn event_of(block: &[String]) -> Option<Event> {
    let mut text = String::new();
    write::push_line(&mut text, "BEGIN:VCALENDAR");
    block.iter().for_each(|line| write::push_line(&mut text, line));
    write::push_line(&mut text, "END:VCALENDAR");
    ical::parse_events(&text, "").into_iter().next()
}

/// `block` with every value of its list properties `name` (`EXDATE`, `RDATE`) mapped by `change`:
/// `None` drops a value. Values that are no time stay as they are. Unchanged when nothing changes;
/// otherwise each time is written on its own line in `zone` ([`write::time_line`]).
fn map_dates(
    block: &[String],
    name: &str,
    zone: Option<Tz>,
    change: impl Fn(EventTime) -> Option<EventTime>,
) -> Vec<String> {
    let mut changed = false;
    let mut lines = Vec::new();
    for (i, prop) in own(block).into_iter().filter(|(_, prop)| prop.name.eq_ignore_ascii_case(name)) {
        let values: Vec<&str> = prop.value.split(',').map(str::trim).collect();
        let times: Vec<Option<EventTime>> = values.iter().map(|value| time_of(&prop, value)).collect();
        if times.iter().any(Option::is_none) {
            // A period or something unreadable: kept as it is, only its readable dates may go.
            let kept: Vec<&str> = values
                .iter()
                .zip(&times)
                .filter(|(_, time)| time.is_none_or(|time| change(time) == Some(time)))
                .map(|(value, _)| *value)
                .collect();
            changed |= kept.len() != values.len();
            if !kept.is_empty() {
                let line = block.get(i).map_or_else(String::new, |line| line.clone());
                let head = line.rsplit_once(':').map_or(line.as_str(), |(head, _)| head).to_string();
                lines.push(format!("{head}:{}", kept.join(",")));
            }
            continue;
        }
        for time in times.into_iter().flatten() {
            let mapped = change(time);
            changed |= mapped != Some(time);
            lines.extend(mapped.map(|time| write::time_line(name, time, zone)));
        }
    }
    if changed { set(block, &[name], lines) } else { block.to_vec() }
}

// ---------------------------------------------------------------------------------------------
// Times

/// A time to compare by: a day counts from its midnight in UTC.
fn instant(time: EventTime) -> DateTime<Utc> {
    match time {
        EventTime::DateTime(at) => at,
        EventTime::Date(day) => day.and_time(NaiveTime::MIN).and_utc(),
    }
}

/// The wall-clock time of `at` in `zone` (UTC without one).
fn wall(at: DateTime<Utc>, zone: Option<Tz>) -> NaiveDateTime {
    zone.map_or_else(|| at.naive_utc(), |tz| at.with_timezone(&tz).naive_local())
}

fn from_wall(naive: NaiveDateTime, zone: Option<Tz>) -> DateTime<Utc> {
    zone.map_or_else(|| naive.and_utc(), |tz| ical::in_zone(tz, naive))
}

/// How far `to` lies from `from` on the clock of `zone` (whole days for days), so a weekly event at
/// 10:00 moved to 11:00 moves every occurrence to 11:00, in summer and in winter. `None` when one is a
/// day and the other a time.
fn shift_between(from: EventTime, to: EventTime, zone: Option<Tz>) -> Option<Duration> {
    match (from, to) {
        (EventTime::Date(from), EventTime::Date(to)) => Some(to - from),
        (EventTime::DateTime(from), EventTime::DateTime(to)) => Some(wall(to, zone) - wall(from, zone)),
        _ => None,
    }
}

/// `time` moved by `by` on the clock of `zone` ([`shift_between`]).
fn shifted(time: EventTime, by: Duration, zone: Option<Tz>) -> EventTime {
    if by.is_zero() {
        return time;
    }
    match time {
        EventTime::Date(day) => EventTime::Date(day + by),
        EventTime::DateTime(at) => EventTime::DateTime(from_wall(wall(at, zone) + by, zone)),
    }
}

/// How long an event from `start` to `end` lasts; `None` without an end.
fn length(start: EventTime, end: Option<EventTime>) -> Option<Duration> {
    end.map(|end| instant(end) - instant(start))
}

/// The end of an event from `start` that lasts `length`.
fn plus(start: EventTime, length: Duration) -> EventTime {
    match start {
        EventTime::Date(day) => EventTime::Date(day + Duration::days(length.num_days())),
        EventTime::DateTime(at) => EventTime::DateTime(at + length),
    }
}

/// The time just before `key`, where a series split there ends (`UNTIL`): the second before, or the
/// day before for an all-day series.
fn just_before(key: EventTime) -> EventTime {
    match key {
        EventTime::Date(day) => EventTime::Date(day - Duration::days(1)),
        EventTime::DateTime(at) => EventTime::DateTime(at - Duration::seconds(1)),
    }
}

// ---------------------------------------------------------------------------------------------
// The series and its dates

/// The main event of a stored series.
#[derive(Debug, Clone)]
struct Main {
    /// As the reader reads it (start, end, fields).
    event: Event,
    /// Its `RRULE` text; `None` without one.
    rule: Option<String>,
    /// The zone of its timed `DTSTART` (`TZID`); `None` for a day, UTC or a zone chrono-tz does not know.
    zone: Option<Tz>,
    /// Its `EXDATE`s.
    excluded: Vec<EventTime>,
    /// Its `RDATE`s.
    extra: Vec<EventTime>,
}

impl Main {
    fn of(block: &[String]) -> Option<Main> {
        let event = event_of(block)?;
        let zone = event.time_zone.as_deref().and_then(ical::zone).filter(|_| !event.all_day());
        Some(Main {
            rule: first(block, "RRULE").map(|prop| prop.value.trim().to_string()),
            zone,
            excluded: all(block, "EXDATE").iter().flat_map(times_of).collect(),
            extra: all(block, "RDATE").iter().flat_map(times_of).collect(),
            event,
        })
    }

    fn is_series(&self) -> bool {
        self.rule.is_some() || !self.extra.is_empty()
    }

    fn length(&self) -> Option<Duration> {
        length(self.event.start, self.event.end)
    }

    /// The main event moved to the occurrence `key`, as the server expands it.
    fn at(&self, key: EventTime) -> Event {
        let end = self.length().map(|length| plus(key, length));
        Event { start: key, end, recurrence_id: Some(key), ..self.event.clone() }
    }

    /// The rule's fields ([`crate::recur::read`]); `None` without a rule.
    fn repeat(&self) -> Option<Repeat> {
        self.rule.as_deref().map(|rule| crate::recur::read(rule, self.zone))
    }

    /// The occurrences of the series in order: the dates of its rule and its extra dates, without the
    /// excluded ones; `None` when the rule cannot be counted.
    fn occurrences(&self) -> Option<Dates> {
        let mut dates = match &self.rule {
            Some(rule) => rule_dates(self.event.start, rule, self.zone)?,
            None => Dates { list: vec![self.event.start], known_until: None },
        };
        dates.list.extend(self.extra.iter().copied());
        dates.list.sort_by_key(|time| instant(*time));
        dates.list.dedup();
        dates.list.retain(|time| !self.excluded.contains(time));
        Some(dates)
    }
}

/// Dates of a series, in order.
#[derive(Debug, Clone)]
struct Dates {
    list: Vec<EventTime>,
    /// Only dates up to here are counted (the rule goes on, or there were too many); `None` when the
    /// list holds every date.
    known_until: Option<DateTime<Utc>>,
}

impl Dates {
    /// Whether it is known if `time` is one of the dates.
    fn knows(&self, time: EventTime) -> bool {
        self.known_until.is_none_or(|until| instant(time) <= until)
    }
}

/// The dates the rule `rule` (an `RRULE` value) gives a series that starts at `start` in `zone`, the
/// start always first (RFC 5545 counts it as the first occurrence), none excluded. `UNTIL` and `COUNT`
/// are read by [`crate::recur::read`], which reads what servers write (an `UNTIL` without `Z`), and
/// handed to the counter as it wants them. `None` when the counter cannot read the rule.
fn rule_dates(start: EventTime, rule: &str, zone: Option<Tz>) -> Option<Dates> {
    let repeat = crate::recur::read(rule, zone);
    let rest: Vec<&str> = rule
        .split(';')
        .map(str::trim)
        .filter(|part| {
            let name = part.split('=').next().unwrap_or_default().trim().to_ascii_uppercase();
            !part.is_empty() && name != "UNTIL" && name != "COUNT"
        })
        .collect();
    let mut parsed: rrule::RRule<rrule::Unvalidated> = rest.join(";").parse().ok()?;
    let tz = match (start, zone) {
        (EventTime::DateTime(_), Some(tz)) => rrule::Tz::Tz(tz),
        _ => rrule::Tz::UTC,
    };
    let horizon = instant(start) + Duration::days(HORIZON_DAYS);
    let mut ends = None;
    match repeat.end {
        RepeatEnd::Count(count) => parsed = parsed.count(count),
        RepeatEnd::Until(until) => {
            let last = match (until, start) {
                (EventTime::Date(day), EventTime::Date(_)) => day.and_time(NaiveTime::MIN).and_utc(),
                (EventTime::Date(day), EventTime::DateTime(_)) => {
                    from_wall(day.and_hms_opt(23, 59, 59).unwrap_or_else(|| day.and_time(NaiveTime::MIN)), zone)
                }
                (EventTime::DateTime(at), _) => at,
            };
            ends = Some(last);
            parsed = parsed.until(last.with_timezone(&rrule::Tz::UTC));
        }
        RepeatEnd::Never => {}
    }
    let set = parsed.build(instant(start).with_timezone(&tz)).ok()?.before(horizon.with_timezone(&tz));
    let result = set.all(LIMIT);
    let made = result.dates.len();
    let mut list: Vec<EventTime> = result
        .dates
        .iter()
        .map(|at| match start {
            EventTime::Date(_) => EventTime::Date(at.with_timezone(&Utc).date_naive()),
            EventTime::DateTime(_) => EventTime::DateTime(at.with_timezone(&Utc)),
        })
        .collect();
    if list.first() != Some(&start) {
        list.insert(0, start);
    }
    let complete = !result.limited
        && match repeat.end {
            RepeatEnd::Count(count) => made >= count as usize,
            RepeatEnd::Until(_) => ends.is_some_and(|last| last < horizon),
            RepeatEnd::Never => false,
        };
    let known_until = match (complete, result.limited) {
        (true, _) => None,
        (false, true) => list.last().map(|time| instant(*time)),
        (false, false) => Some(horizon),
    };
    Some(Dates { list, known_until })
}

/// `rule` (an `RRULE` value) with its end replaced by `end` (`COUNT=4`, `UNTIL=20261027T085959Z`).
fn with_end(rule: &str, end: &str) -> String {
    let mut parts: Vec<&str> = rule
        .split(';')
        .map(str::trim)
        .filter(|part| {
            let name = part.split('=').next().unwrap_or_default().trim().to_ascii_uppercase();
            !part.is_empty() && name != "UNTIL" && name != "COUNT"
        })
        .collect();
    parts.push(end);
    parts.join(";")
}

/// `UNTIL=` with `time` as RFC 5545 wants it: a day for an all-day series, else in UTC.
fn until_part(time: EventTime) -> String {
    match time {
        EventTime::Date(day) => format!("UNTIL={}", day.format("%Y%m%d")),
        EventTime::DateTime(at) => format!("UNTIL={}", ical::format_utc(at)),
    }
}

/// The rule of the part of `main`'s series from `key` on, when it is split there: the same rule, with
/// a `COUNT` reduced by the dates before `key`. `NoOccurrence` when none are left.
fn rest_rule(main: &Main, key: EventTime) -> Result<Option<String>, SeriesError> {
    let Some(rule) = &main.rule else { return Ok(None) };
    let Some(RepeatEnd::Count(count)) = main.repeat().map(|repeat| repeat.end) else { return Ok(Some(rule.clone())) };
    let dates = rule_dates(main.event.start, rule, main.zone).ok_or(SeriesError::Unsupported(
        "the rule of this series cannot be counted, so it cannot be split: change all events or one",
    ))?;
    let before = dates.list.iter().filter(|time| instant(**time) < instant(key)).count();
    let rest = (count as usize).checked_sub(before).filter(|rest| *rest > 0).ok_or(SeriesError::NoOccurrence)?;
    Ok(Some(with_end(rule, &format!("COUNT={rest}"))))
}

// ---------------------------------------------------------------------------------------------
// Finding the occurrence

/// What the stored object says about the occurrence the client changes.
struct Found {
    object: Object,
    main: Option<Main>,
    /// The occurrence as stored now: its changed occurrence, else the main event at its key; the
    /// event itself for a single event.
    before: Event,
    /// Its key; `None` for a single event.
    key: Option<EventTime>,
}

impl Found {
    /// `key` is the first occurrence: the main event's own start.
    fn is_first(&self, key: EventTime) -> bool {
        self.main.as_ref().is_some_and(|main| main.event.start == key)
    }
}

fn find(stored: &str, event: &Event) -> Result<Found, SeriesError> {
    let object = Object::read(stored);
    let main = object.master().and_then(|block| Main::of(block));
    let series = main.as_ref().is_some_and(Main::is_series) || !object.exception_keys().is_empty();
    if !series {
        let before = main.as_ref().map(|main| main.event.clone()).ok_or(SeriesError::NoOccurrence)?;
        return Ok(Found { object, main, before, key: None });
    }
    let key = event.recurrence_id.ok_or(SeriesError::NoOccurrence)?;
    if let Some(before) = object.exception(key).and_then(|block| event_of(block)) {
        return Ok(Found { object, main, before, key: Some(key) });
    }
    let held = main.as_ref().ok_or(SeriesError::NoOccurrence)?;
    // A key the series does not have would make an extra event on the server.
    if let Some(dates) = held.occurrences()
        && dates.knows(key)
        && !dates.list.contains(&key)
    {
        return Err(SeriesError::NoOccurrence);
    }
    let before = held.at(key);
    Ok(Found { object, main, before, key: Some(key) })
}

/// How a save changes the rule of the series.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Rule {
    /// It stays.
    Keep,
    /// It becomes this text.
    Set(String),
    /// The series ends: no rule, no extra or excluded dates.
    Remove,
}

fn rule_change(main: &Main, event: &Event) -> Rule {
    match event.repeat.as_deref() {
        None if main.is_series() => Rule::Remove,
        None => Rule::Keep,
        Some(repeat) => {
            let text = write::rule_text(repeat, main.rule.as_deref(), main.event.start, main.zone);
            if text.is_empty() || Some(text.as_str()) == main.rule.as_deref() { Rule::Keep } else { Rule::Set(text) }
        }
    }
}

/// `onto` with every field `to` changed against `from` set to `to`'s value: title, place, notes,
/// free or busy, link, status, reminders, zone. With `unless_own`, only where `onto` still has the
/// value `unless_own` has, so a changed occurrence keeps what it changed itself.
fn carry(onto: &mut Event, from: &Event, to: &Event, unless_own: Option<&Event>) {
    macro_rules! carry_fields {
        ($($field:ident),*) => {$(
            if to.$field != from.$field && unless_own.is_none_or(|own| onto.$field == own.$field) {
                onto.$field = to.$field.clone();
            }
        )*};
    }
    carry_fields!(summary, location, description, transparency, url, status, geo, alarms, time_zone);
}

// ---------------------------------------------------------------------------------------------
// Saving

fn save_occurrence(found: Found, key: EventTime, event: &Event, writing: &Writing<'_>) -> Result<Outcome, SeriesError> {
    let mut object = found.object;
    let zone = found.main.as_ref().and_then(|main| main.zone);
    let block = match object.exception(key) {
        Some(block) => block.clone(),
        None => {
            let main = object.master().ok_or(SeriesError::NoOccurrence)?;
            let mut lines = vec![write::time_line("DTSTART", key, zone)];
            lines.extend(found.before.end.map(|end| write::time_line("DTEND", end, zone)));
            lines.push(write::time_line("RECURRENCE-ID", key, zone));
            let names = [&RULE_PROPERTIES[..], &["DTSTART", "DTEND", "DURATION", "RECURRENCE-ID"]].concat();
            set(main, &names, lines)
        }
    };
    let uid = first(&block, "UID").map_or_else(|| event.uid.clone(), |prop| prop.value.trim().to_string());
    let target = Event { uid, repeat: None, recurrence_id: Some(key), ..event.clone() };
    let (block, zone_block) = write::event_onto_block(&block, &target, &object.zones(), writing.now, writing.local);
    object.put_exception(key, block);
    object.add_zone(zone_block);
    Ok(Outcome { plan: Plan::Replace { ics: object.text() }, reset: 0, key: Some(key) })
}

fn save_series(found: Found, key: EventTime, event: &Event, writing: &Writing<'_>) -> Result<Outcome, SeriesError> {
    let main = found.main.as_ref().ok_or(SeriesError::Unsupported(NO_MAIN))?;
    let rule = rule_change(main, event);
    let mut object = found.object;
    let edited = edit_series(&mut object, &found.before, key, event, &rule, writing)?;
    Ok(Outcome { plan: Plan::Replace { ics: object.text() }, reset: edited.reset, key: edited.key })
}

fn save_following(found: Found, key: EventTime, event: &Event, writing: &Writing<'_>) -> Result<Outcome, SeriesError> {
    let main = found.main.as_ref().ok_or(SeriesError::Unsupported(NO_MAIN))?;
    let rule = rule_change(main, event);
    let shift =
        shift_between(found.before.start, event.start, main.zone).ok_or(SeriesError::Unsupported(KIND_CHANGE))?;
    let resized = length(found.before.start, found.before.end) != length(event.start, event.end);
    let head = head(&found.object, main, key, writing.now);
    let mut tail = tail(&found.object, main, key, writing.new_uid)?;
    let mut reset = 0;
    // Nextcloud's rule (decision 4): later changed occurrences move only while the dates stay.
    if !shift.is_zero() || resized || rule != Rule::Keep {
        reset = tail.exception_keys().iter().filter(|other| **other != key).count() as u32;
        tail.retain_vevents(is_master);
        tail.edit_master(|block| set(block, &["EXDATE", "RDATE"], Vec::new()));
    }
    let edited = edit_series(&mut tail, &found.before, key, event, &rule, writing)?;
    let plan = Plan::Split { new: tail.text(), old: head.text() };
    Ok(Outcome { plan, reset: reset + edited.reset, key: edited.key })
}

/// The stored series ending just before `key`: `UNTIL` the second (all-day: the day) before, without
/// the extra and excluded dates and the changed occurrences from `key` on.
fn head(object: &Object, main: &Main, key: EventTime, now: DateTime<Utc>) -> Object {
    let mut head = object.clone();
    let later = |time: EventTime| instant(time) >= instant(key);
    head.retain_vevents(|block| key_of(block).is_none_or(|other| !later(other)));
    head.edit_master(|block| {
        let mut block = match &main.rule {
            Some(rule) => {
                let rule = with_end(rule, &until_part(just_before(key)));
                set(block, &["RRULE"], vec![format!("RRULE:{rule}")])
            }
            None => block.to_vec(),
        };
        for name in ["EXDATE", "RDATE"] {
            block = map_dates(&block, name, main.zone, |time| (!later(time)).then_some(time));
        }
        touch(&block, now)
    });
    head
}

/// The series from `key` on as a new object: every `VEVENT` with the UID `uid`, the main event
/// starting at `key` with the rest of a `COUNT` and `RELATED-TO;RELTYPE=SIBLING` to the stored series,
/// the extra and excluded dates and changed occurrences from `key` on.
fn tail(object: &Object, main: &Main, key: EventTime, uid: &str) -> Result<Object, SeriesError> {
    let rule = rest_rule(main, key)?;
    let mut tail = object.clone();
    let later = |time: EventTime| instant(time) >= instant(key);
    tail.retain_vevents(|block| key_of(block).is_none_or(later));
    for block in tail.vevents_mut() {
        *block = set(block, &["UID"], vec![format!("UID:{uid}")]);
    }
    let start = main.at(key);
    tail.edit_master(|block| {
        let mut lines = vec![write::time_line("DTSTART", key, main.zone)];
        lines.extend(start.end.map(|end| write::time_line("DTEND", end, main.zone)));
        lines.extend(rule.map(|rule| format!("RRULE:{rule}")));
        lines.push(format!("RELATED-TO;RELTYPE=SIBLING:{}", main.event.uid));
        let mut block = set(block, &["DTSTART", "DTEND", "DURATION", "RRULE"], lines);
        for name in ["EXDATE", "RDATE"] {
            block = map_dates(&block, name, main.zone, |time| later(time).then_some(time));
        }
        block
    });
    Ok(tail)
}

/// What [`edit_series`] did.
struct Edited {
    reset: u32,
    key: Option<EventTime>,
}

/// Changes the series in `object` as `event` changes `before`, its occurrence with the key `key`
/// (decision 5): the main event gets the changed fields and a start moved by the same distance on the
/// clock of the series, the changed occurrences their moved keys, the changes they did not make
/// themselves, and the moved start when they had not moved; `EXDATE` and `RDATE` move along. A new
/// rule drops the changed occurrences and excluded dates it does not have; no rule (`Rule::Remove`)
/// leaves the occurrence the client changed as a single event.
fn edit_series(
    object: &mut Object,
    before: &Event,
    key: EventTime,
    event: &Event,
    rule: &Rule,
    writing: &Writing<'_>,
) -> Result<Edited, SeriesError> {
    let main = object.master().and_then(|block| Main::of(block)).ok_or(SeriesError::Unsupported(NO_MAIN))?;
    let zone = main.zone;
    let shift = shift_between(before.start, event.start, zone).ok_or(SeriesError::Unsupported(KIND_CHANGE))?;
    let resized = length(before.start, before.end) != length(event.start, event.end);
    let old_length = main.length();
    let new_length = if resized { length(event.start, event.end) } else { old_length };

    let mut target = main.event.clone();
    carry(&mut target, before, event, None);
    target.recurrence_id = None;
    match rule {
        Rule::Remove => {
            target.start = event.start;
            target.end = event.end;
            target.repeat = None;
        }
        Rule::Keep | Rule::Set(_) => {
            target.start = shifted(main.event.start, shift, zone);
            target.end = new_length.map(|length| plus(target.start, length));
            let repeat = match rule {
                Rule::Set(text) => Some(Repeat { custom: true, ..crate::recur::read(text, zone) }),
                _ => main.repeat(),
            };
            target.repeat = repeat.map(Box::new);
        }
    }
    // Which dates the series has afterwards, to tell which keys stay.
    let new_dates = match rule {
        Rule::Keep if shift.is_zero() => None,
        Rule::Remove => Some(Dates { list: vec![target.start], known_until: None }),
        Rule::Keep | Rule::Set(_) => {
            let rule = match rule {
                Rule::Set(text) => Some(text.clone()),
                _ => main.rule.clone(),
            };
            let extra = main.extra.iter().map(|time| shifted(*time, shift, zone)).collect();
            Main { event: target.clone(), rule, zone, excluded: Vec::new(), extra }.occurrences()
        }
    };
    let stays = |old: EventTime| {
        let new = shifted(old, shift, zone);
        match (&new_dates, rule) {
            (_, Rule::Remove) => false,
            (None, _) => true,
            (Some(dates), _) => !dates.knows(new) || dates.list.contains(&new),
        }
    };

    let mut defined = object.zones();
    let mut zones = Vec::new();
    let mut reset = 0;
    let mut merge = |block: &[String], target: &Event, zones: &mut Vec<Vec<String>>| {
        let (block, zone_block) = write::event_onto_block(block, target, &defined, writing.now, writing.local);
        if let Some(name) = zone_block.iter().find_map(|line| line.strip_prefix("TZID:")) {
            defined.push(name.to_string());
            zones.push(zone_block);
        }
        block
    };
    let mut kept = Vec::new();
    for part in std::mem::take(&mut object.parts) {
        let Part::Vevent(block) = part else {
            kept.push(part);
            continue;
        };
        if is_master(&block) {
            // A later main event (which no reader takes for the series) stays as it is.
            if kept.iter().any(|part| matches!(part, Part::Vevent(b) if is_master(b))) {
                kept.push(Part::Vevent(block));
                continue;
            }
            let mut lines = block;
            match rule {
                Rule::Remove => lines = set(&lines, &["RDATE", "EXDATE", "EXRULE"], Vec::new()),
                Rule::Keep | Rule::Set(_) => {
                    lines = map_dates(&lines, "EXDATE", zone, |time| stays(time).then(|| shifted(time, shift, zone)));
                    lines = map_dates(&lines, "RDATE", zone, |time| Some(shifted(time, shift, zone)));
                }
            }
            kept.push(Part::Vevent(merge(&lines, &target, &mut zones)));
            continue;
        }
        let (Some(old), Some(mut changed)) = (key_of(&block), event_of(&block)) else {
            kept.push(Part::Vevent(block));
            continue;
        };
        if !stays(old) {
            reset += u32::from(old != key);
            continue;
        }
        let new = shifted(old, shift, zone);
        if old == key {
            carry(&mut changed, before, event, None);
            changed.start = event.start;
            changed.end = event.end;
        } else {
            let own_length = length(changed.start, changed.end);
            carry(&mut changed, before, event, Some(&main.event));
            if changed.start == old {
                changed.start = new;
                let length = if resized && own_length == old_length { new_length } else { own_length };
                changed.end = length.map(|length| plus(new, length));
            }
        }
        changed.repeat = None;
        changed.recurrence_id = Some(new);
        changed.uid = main.event.uid.clone();
        let lines = if new == old {
            block
        } else {
            set(&block, &["RECURRENCE-ID"], vec![write::time_line("RECURRENCE-ID", new, zone)])
        };
        kept.push(Part::Vevent(merge(&lines, &changed, &mut zones)));
    }
    object.parts = kept;
    zones.into_iter().for_each(|zone| object.add_zone(zone));
    let key = match rule {
        Rule::Remove => None,
        Rule::Keep | Rule::Set(_) => Some(shifted(key, shift, zone)),
    };
    Ok(Edited { reset, key })
}

// ---------------------------------------------------------------------------------------------
// What a save drops

/// What a save removes from a series ([`drops`], [`resets`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Lost {
    dropped: u32,
    reset: u32,
}

fn lost(stored: &str, event: &Event, scope: SeriesScope) -> Result<Lost, SeriesError> {
    if scope == SeriesScope::Occurrence {
        return Ok(Lost::default());
    }
    let found = find(stored, event)?;
    // A single event: a new rule only adds events.
    let Some(key) = found.key else { return Ok(Lost::default()) };
    let main = found.main.as_ref().ok_or(SeriesError::Unsupported(NO_MAIN))?;
    let zone = main.zone;
    let rule = rule_change(main, event);
    let following = scope == SeriesScope::Following && !found.is_first(key);
    let shift = shift_between(found.before.start, event.start, zone).ok_or(SeriesError::Unsupported(KIND_CHANGE))?;
    let resized = length(found.before.start, found.before.end) != length(event.start, event.end);
    let from = |time: EventTime| !following || instant(time) >= instant(key);
    if rule == Rule::Keep && shift.is_zero() && !resized {
        return Ok(Lost::default());
    }

    // The series as it would be: its start, rule and extra dates.
    let start = match rule {
        Rule::Remove => event.start,
        Rule::Keep | Rule::Set(_) => shifted(if following { key } else { main.event.start }, shift, zone),
    };
    let new_rule = match &rule {
        Rule::Remove => None,
        Rule::Set(text) => Some(text.clone()),
        Rule::Keep if following => rest_rule(main, key)?,
        Rule::Keep => main.rule.clone(),
    };
    let extra = match rule {
        Rule::Remove => Vec::new(),
        Rule::Keep | Rule::Set(_) => {
            main.extra.iter().filter(|time| from(**time)).map(|time| shifted(*time, shift, zone)).collect()
        }
    };
    let new_event = Event { start, ..main.event.clone() };
    let new = Main { event: new_event, rule: new_rule, zone, excluded: Vec::new(), extra }.occurrences();

    let changed: Vec<EventTime> =
        found.object.exception_keys().into_iter().filter(|other| from(*other) && *other != key).collect();
    let reset: Vec<EventTime> = match (&rule, &new) {
        (Rule::Remove, _) => changed,
        // After a split with new times or a new rule, the later changed occurrences are reset.
        _ if following => changed,
        (_, Some(new)) => changed
            .into_iter()
            .filter(|old| {
                let moved = shifted(*old, shift, zone);
                new.knows(moved) && !new.list.contains(&moved)
            })
            .collect(),
        (_, None) => changed,
    };
    let mut gone: Vec<EventTime> = match (main.occurrences(), &new) {
        (Some(old), Some(new)) => old
            .list
            .into_iter()
            .filter(|old| from(*old))
            .filter(|old| {
                let moved = shifted(*old, shift, zone);
                new.knows(moved) && !new.list.contains(&moved)
            })
            .collect(),
        // Nothing to count with: whatever changes may drop something.
        _ => vec![key],
    };
    for old in &reset {
        if !gone.contains(old) {
            gone.push(*old);
        }
    }
    let mut dropped = gone.len() as u32;
    if rule == Rule::Remove {
        dropped = dropped.max(1);
    }
    Ok(Lost { dropped, reset: reset.len() as u32 })
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone};
    use torro_pim::model::RepeatFreq;

    use super::*;

    const EXCEPTIONS: &str = include_str!("../tests/fixtures/nextcloud-series-exceptions.ics");
    const COUNTED: &str = include_str!("../tests/fixtures/nextcloud-series-count.ics");
    const ALL_DAY: &str = include_str!("../tests/fixtures/nextcloud-series-all-day.ics");
    const DST: &str = include_str!("../tests/fixtures/nextcloud-series-dst.ics");
    const NEW_UID: &str = "new-series@torro";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 22, 12, 0, 0).single().expect("now")
    }

    fn writing() -> Writing<'static> {
        Writing { now: now(), local: Some(Tz::Europe__Berlin), new_uid: NEW_UID }
    }

    /// 10:00 on `day` in Berlin.
    fn berlin(year: i32, month: u32, day: u32, hour: u32) -> EventTime {
        let naive = NaiveDate::from_ymd_opt(year, month, day).and_then(|d| d.and_hms_opt(hour, 0, 0)).expect("time");
        EventTime::DateTime(ical::in_zone(Tz::Europe__Berlin, naive))
    }

    fn day(year: i32, month: u32, day: u32) -> EventTime {
        EventTime::Date(NaiveDate::from_ymd_opt(year, month, day).expect("day"))
    }

    /// The occurrence with key `key` as the core caches it: as the server expands it, with the rule
    /// and zone of the series.
    fn occurrence(stored: &str, key: EventTime) -> Event {
        let found = Object::read(stored);
        let main = found.master().and_then(|block| Main::of(block)).expect("a main event");
        let mut event = match found.exception(key).and_then(|block| event_of(block)) {
            Some(changed) => changed,
            None => main.at(key),
        };
        event.recurrence_id = Some(key);
        event.repeat = main.repeat().map(Box::new);
        event.time_zone = main.event.time_zone.clone();
        event.recurring = true;
        event.href = Some("https://cloud.example.org/remote.php/dav/calendars/anna/work/x.ics".into());
        event.etag = Some("\"1\"".into());
        event
    }

    fn replaced_ics(outcome: &Outcome) -> &str {
        match &outcome.plan {
            Plan::Replace { ics } => ics,
            other => panic!("expected a replace, got {other:?}"),
        }
    }

    fn split(outcome: &Outcome) -> (&str, &str) {
        match &outcome.plan {
            Plan::Split { new, old } => (new, old),
            other => panic!("expected a split, got {other:?}"),
        }
    }

    /// The events of `ics` as a reader reads them, main event first.
    fn events(ics: &str) -> Vec<Event> {
        ical::parse_events(ics, "")
    }

    fn lines(ics: &str) -> Vec<String> {
        ical::unfold(ics)
    }

    fn has(ics: &str, line: &str) -> bool {
        lines(ics).iter().any(|l| l == line)
    }

    fn count(ics: &str, prefix: &str) -> usize {
        lines(ics).iter().filter(|l| l.starts_with(prefix)).count()
    }

    /// What a save leaves for other programs: their properties and alarms.
    fn keeps_foreign_lines(ics: &str) {
        for line in ["COLOR:steelblue", "X-NC-GROUP-ID:product", "CATEGORIES:Work", "X-WR-ALARMUID:7d1c0b2e-alarm"] {
            assert!(has(ics, line), "{line} stays: {ics}");
        }
        assert_eq!(count(ics, "BEGIN:VTIMEZONE"), 1, "one zone, as stored");
    }

    #[test]
    fn the_fixtures_read_as_series() {
        let main = Object::read(EXCEPTIONS).master().and_then(|b| Main::of(b)).expect("main");
        let dates = main.occurrences().expect("countable");
        assert_eq!(dates.list.first(), Some(&berlin(2026, 9, 1, 10)));
        assert!(!dates.list.contains(&berlin(2026, 9, 22, 10)), "excluded");
        assert!(dates.list.contains(&berlin(2026, 10, 27, 10)), "after the change to winter time at 10:00 too");
        assert!(dates.known_until.is_some(), "the rule goes on");
        let counted =
            Object::read(COUNTED).master().and_then(|b| Main::of(b)).and_then(|m| m.occurrences()).expect("dates");
        assert_eq!((counted.list.len(), counted.known_until), (10, None));
        let all_day =
            Object::read(ALL_DAY).master().and_then(|b| Main::of(b)).and_then(|m| m.occurrences()).expect("dates");
        assert_eq!(all_day.list.first(), Some(&day(2026, 10, 2)));
        assert_eq!(all_day.list.last(), Some(&day(2026, 11, 27)), "UNTIL as a day counts");
        assert!(!all_day.list.contains(&day(2026, 10, 16)));
    }

    // One occurrence (decision 3).

    #[test]
    fn changing_one_occurrence_adds_a_changed_occurrence_and_keeps_the_others() {
        let key = berlin(2026, 10, 13, 10);
        let event = Event { summary: "Product sync with design".into(), ..occurrence(EXCEPTIONS, key) };
        let outcome = save(EXCEPTIONS, &event, SeriesScope::Occurrence, &writing()).expect("saved");
        assert_eq!((outcome.reset, outcome.key), (0, Some(key)));
        let ics = replaced_ics(&outcome);
        keeps_foreign_lines(ics);
        let read = events(ics);
        assert_eq!(read.len(), 4, "the main event, the two changed ones and the new one");
        let new = read.iter().find(|e| e.recurrence_id == Some(key)).expect("the new changed occurrence");
        assert_eq!((new.summary.as_str(), new.start), ("Product sync with design", key));
        assert_eq!(new.alarms, read[0].alarms, "a copy of the main event");
        assert!(has(ics, "RECURRENCE-ID;TZID=Europe/Berlin:20261013T100000"));
        assert_eq!(count(ics, "RRULE:FREQ=WEEKLY"), 1, "the rule stays with the main event only");
        assert_eq!(count(ics, "EXDATE"), 1);
        assert_eq!(read[0].summary, "Weekly product sync", "the series is untouched");
        assert!(read.iter().any(|e| e.summary == "Weekly product sync (Q4 review)"));
    }

    #[test]
    fn changing_a_changed_occurrence_replaces_it() {
        let key = berlin(2026, 10, 6, 10);
        let stored = occurrence(EXCEPTIONS, key);
        assert_eq!(stored.start, berlin(2026, 10, 6, 11), "moved to 11:00");
        let event = Event { location: Some("Room Alster".into()), ..stored };
        let outcome = save(EXCEPTIONS, &event, SeriesScope::Occurrence, &writing()).expect("saved");
        let read = events(replaced_ics(&outcome));
        assert_eq!(read.len(), 3, "no new component");
        let changed = read.iter().find(|e| e.recurrence_id == Some(key)).expect("still there");
        assert_eq!((changed.location.as_deref(), changed.start), (Some("Room Alster"), berlin(2026, 10, 6, 11)));
    }

    #[test]
    fn deleting_one_occurrence_excludes_its_date_in_the_zone_of_the_start() {
        let key = berlin(2026, 10, 6, 10);
        let outcome = delete(EXCEPTIONS, &occurrence(EXCEPTIONS, key), SeriesScope::Occurrence, now()).expect("ok");
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "EXDATE;TZID=Europe/Berlin:20261006T100000"));
        assert!(has(ics, "EXDATE;TZID=Europe/Berlin:20260922T100000"), "the old one stays");
        assert_eq!(events(ics).len(), 2, "its changed occurrence is gone, the other stays");
        keeps_foreign_lines(ics);
        // An all-day series excludes a day.
        let outcome =
            delete(ALL_DAY, &occurrence(ALL_DAY, day(2026, 10, 9)), SeriesScope::Occurrence, now()).expect("ok");
        assert!(has(replaced_ics(&outcome), "EXDATE;VALUE=DATE:20261009"));
    }

    #[test]
    fn an_occurrence_needs_its_key_and_the_key_must_be_one_of_the_series() {
        let mut event = occurrence(EXCEPTIONS, berlin(2026, 10, 13, 10));
        event.recurrence_id = None;
        assert_eq!(save(EXCEPTIONS, &event, SeriesScope::Occurrence, &writing()), Err(SeriesError::NoOccurrence));
        event.recurrence_id = Some(berlin(2026, 10, 14, 10));
        assert_eq!(
            save(EXCEPTIONS, &event, SeriesScope::Series, &writing()),
            Err(SeriesError::NoOccurrence),
            "a Wednesday"
        );
        event.recurrence_id = Some(berlin(2026, 9, 22, 10));
        assert_eq!(
            delete(EXCEPTIONS, &event, SeriesScope::Occurrence, now()),
            Err(SeriesError::NoOccurrence),
            "excluded"
        );
    }

    // The whole series (decision 5).

    #[test]
    fn renaming_the_series_renames_the_changed_occurrences_that_kept_the_title() {
        let key = berlin(2026, 10, 13, 10);
        let event = Event { summary: "Product sync".into(), ..occurrence(EXCEPTIONS, key) };
        let outcome = save(EXCEPTIONS, &event, SeriesScope::Series, &writing()).expect("saved");
        assert_eq!((outcome.reset, outcome.key), (0, Some(key)));
        let ics = replaced_ics(&outcome);
        keeps_foreign_lines(ics);
        let read = events(ics);
        let titles: Vec<&str> = read.iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(titles, ["Product sync", "Product sync", "Weekly product sync (Q4 review)"]);
        assert_eq!(read[1].start, berlin(2026, 10, 6, 11), "its own time stays");
        assert!(has(ics, "RRULE:FREQ=WEEKLY;BYDAY=TU"), "the rule as stored");
        assert!(has(ics, "EXDATE;TZID=Europe/Berlin:20260922T100000"));
        assert_eq!(drops(EXCEPTIONS, &event, SeriesScope::Series), 0, "a new title drops nothing");
    }

    #[test]
    fn moving_the_series_moves_keys_and_excluded_dates_on_the_clock_of_its_zone() {
        // 10:00 → 11:00 on 13 Oct (summer time); the occurrence on 27 Oct (winter time) moves to
        // 11:00 as well, not to 10:00.
        let key = berlin(2026, 10, 13, 10);
        let stored = occurrence(EXCEPTIONS, key);
        let event = Event { start: berlin(2026, 10, 13, 11), end: Some(berlin(2026, 10, 13, 12)), ..stored };
        let outcome = save(EXCEPTIONS, &event, SeriesScope::Series, &writing()).expect("saved");
        assert_eq!((outcome.reset, outcome.key), (0, Some(berlin(2026, 10, 13, 11))));
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "DTSTART;TZID=Europe/Berlin:20260901T110000"));
        assert!(has(ics, "EXDATE;TZID=Europe/Berlin:20260922T110000"));
        assert!(has(ics, "RECURRENCE-ID;TZID=Europe/Berlin:20261006T110000"));
        assert!(has(ics, "RECURRENCE-ID;TZID=Europe/Berlin:20261027T110000"));
        let read = events(ics);
        assert_eq!(read[1].start, berlin(2026, 10, 6, 11), "moved by itself already: it stays");
        assert_eq!(read[2].start, berlin(2026, 10, 27, 11), "not moved by itself: it moves along");
        assert_eq!(read[2].description.as_deref(), Some("Q4 numbers first."), "its own notes stay");
        keeps_foreign_lines(ics);
        assert_eq!(drops(EXCEPTIONS, &event, SeriesScope::Series), 0, "moving drops nothing");
    }

    #[test]
    fn a_new_rule_resets_the_changed_occurrences_it_no_longer_has() {
        let key = berlin(2026, 10, 13, 10);
        let stored = occurrence(EXCEPTIONS, key);
        let mut repeat = stored.repeat.as_deref().cloned().expect("a rule");
        repeat.interval = 2;
        let event = Event { repeat: Some(Box::new(repeat)), ..stored };
        // Every other Tuesday from 1 Sep: 13 Oct and 27 Oct stay, 6 Oct goes.
        let outcome = save(EXCEPTIONS, &event, SeriesScope::Series, &writing()).expect("saved");
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=TU"));
        assert_eq!(outcome.reset, 1);
        assert_eq!(resets(EXCEPTIONS, &event, SeriesScope::Series), 1);
        assert!(drops(EXCEPTIONS, &event, SeriesScope::Series) > 0, "every other week goes");
        let keys: Vec<Option<EventTime>> = events(ics).iter().map(|e| e.recurrence_id).collect();
        assert_eq!(keys, [None, Some(berlin(2026, 10, 27, 10))]);
        assert!(!has(ics, "EXDATE;TZID=Europe/Berlin:20260922T100000"), "22 Sep is no date of the new rule");
    }

    #[test]
    fn no_rule_makes_the_changed_occurrence_a_single_event() {
        let key = berlin(2026, 10, 13, 10);
        let event = Event { repeat: None, ..occurrence(EXCEPTIONS, key) };
        let outcome = save(EXCEPTIONS, &event, SeriesScope::Series, &writing()).expect("saved");
        assert_eq!((outcome.reset, outcome.key), (2, None));
        let ics = replaced_ics(&outcome);
        let read = events(ics);
        assert_eq!(read.len(), 1);
        assert_eq!((read[0].start, read[0].recurrence_id), (key, None));
        for gone in ["RRULE:FREQ=WEEKLY", "EXDATE", "RECURRENCE-ID"] {
            assert_eq!(count(ics, gone), 0, "{gone}");
        }
        assert!(drops(EXCEPTIONS, &event, SeriesScope::Series) > 2);
        assert_eq!(drops(EXCEPTIONS, &event, SeriesScope::Occurrence), 0, "one occurrence never ends a series");
    }

    #[test]
    fn a_shorter_count_drops_events_and_a_longer_one_does_not() {
        let key = berlin(2026, 10, 15, 8);
        let stored = occurrence(COUNTED, key);
        let shorter = |n| {
            let mut repeat = stored.repeat.as_deref().cloned().expect("rule");
            repeat.end = RepeatEnd::Count(n);
            Event { repeat: Some(Box::new(repeat)), ..stored.clone() }
        };
        assert_eq!(drops(COUNTED, &shorter(7), SeriesScope::Series), 3);
        assert_eq!(drops(COUNTED, &shorter(12), SeriesScope::Series), 0);
        let outcome = save(COUNTED, &shorter(7), SeriesScope::Series, &writing()).expect("saved");
        assert!(has(replaced_ics(&outcome), "RRULE:FREQ=DAILY;COUNT=7"));
        assert!(has(replaced_ics(&outcome), "X-NC-GROUP-ID:sport"));
    }

    #[test]
    fn an_all_day_series_moves_by_days() {
        let key = day(2026, 10, 9);
        let event = Event { start: day(2026, 10, 10), end: Some(day(2026, 10, 11)), ..occurrence(ALL_DAY, key) };
        let mut moved = event.clone();
        let mut repeat = moved.repeat.as_deref().cloned().expect("rule");
        repeat.weekdays = vec![torro_pim::model::RepeatDay { day: torro_pim::model::Weekday::Saturday, nth: None }];
        moved.repeat = Some(Box::new(repeat));
        let outcome = save(ALL_DAY, &moved, SeriesScope::Series, &writing()).expect("saved");
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "DTSTART;VALUE=DATE:20261003"));
        assert!(has(ics, "DTEND;VALUE=DATE:20261004"));
        assert!(has(ics, "RRULE:FREQ=WEEKLY;BYDAY=SA;UNTIL=20261127"));
        assert!(has(ics, "EXDATE;VALUE=DATE:20261017"));
        assert!(has(ics, "RECURRENCE-ID;VALUE=DATE:20261024"));
        assert_eq!((outcome.reset, outcome.key), (0, Some(day(2026, 10, 10))));
        // The same days a day later, but the end stays: the last Friday (27 Nov) has no Saturday left.
        assert_eq!(drops(ALL_DAY, &moved, SeriesScope::Series), 1);
        assert!(has(ics, "X-NC-GROUP-ID:focus"));
        // Changing between all-day and timed is not written.
        let timed =
            Event { start: berlin(2026, 10, 9, 9), end: Some(berlin(2026, 10, 9, 10)), ..occurrence(ALL_DAY, key) };
        assert!(matches!(save(ALL_DAY, &timed, SeriesScope::Series, &writing()), Err(SeriesError::Unsupported(_))));
        assert!(save(ALL_DAY, &timed, SeriesScope::Occurrence, &writing()).is_ok(), "one occurrence may");
    }

    #[test]
    fn deleting_the_series_deletes_the_object() {
        let event = occurrence(EXCEPTIONS, berlin(2026, 10, 13, 10));
        assert_eq!(delete(EXCEPTIONS, &event, SeriesScope::Series, now()).map(|o| o.plan), Ok(Plan::Delete));
    }

    // This and all following (decisions 4 and 6).

    #[test]
    fn following_splits_and_later_changed_occurrences_move_into_the_new_series() {
        let key = berlin(2026, 10, 13, 10);
        let event = Event { summary: "Product sync, new format".into(), ..occurrence(EXCEPTIONS, key) };
        let outcome = save(EXCEPTIONS, &event, SeriesScope::Following, &writing()).expect("saved");
        assert_eq!((outcome.reset, outcome.key), (0, Some(key)));
        let (new, old) = split(&outcome);
        // The old series ends the second before, in UTC: 13 Oct 10:00 in Berlin is 08:00 UTC.
        assert!(has(old, "RRULE:FREQ=WEEKLY;BYDAY=TU;UNTIL=20261013T075959Z"), "{old}");
        let old_events = events(old);
        assert_eq!(old_events.len(), 2, "the main event and the change of 6 Oct");
        assert_eq!(old_events[0].summary, "Weekly product sync");
        assert!(has(old, "EXDATE;TZID=Europe/Berlin:20260922T100000"));
        keeps_foreign_lines(old);

        let new_events = events(new);
        assert_eq!(new_events.len(), 2, "the main event and the change of 27 Oct");
        assert!(new_events.iter().all(|e| e.uid == NEW_UID));
        assert_eq!((new_events[0].start, new_events[0].summary.as_str()), (key, "Product sync, new format"));
        assert!(has(new, "RRULE:FREQ=WEEKLY;BYDAY=TU"));
        assert!(has(new, "RELATED-TO;RELTYPE=SIBLING:4f2e8c1a-6b7d-4e3f-9a10-2c5d7e9f1b24"));
        assert_eq!(count(new, "EXDATE"), 0, "22 Sep stays with the old series");
        assert_eq!(new_events[1].summary, "Weekly product sync (Q4 review)", "its own title stays");
        keeps_foreign_lines(new);
        assert_eq!(drops(EXCEPTIONS, &event, SeriesScope::Following), 0);
    }

    #[test]
    fn following_with_a_new_time_resets_the_later_changed_occurrences() {
        let key = berlin(2026, 10, 13, 10);
        let stored = occurrence(EXCEPTIONS, key);
        let event = Event { start: berlin(2026, 10, 13, 9), end: Some(berlin(2026, 10, 13, 10)), ..stored };
        assert_eq!(resets(EXCEPTIONS, &event, SeriesScope::Following), 1);
        assert_eq!(drops(EXCEPTIONS, &event, SeriesScope::Following), 1, "the changed one of 27 Oct");
        let outcome = save(EXCEPTIONS, &event, SeriesScope::Following, &writing()).expect("saved");
        assert_eq!((outcome.reset, outcome.key), (1, Some(berlin(2026, 10, 13, 9))));
        let (new, _) = split(&outcome);
        let new_events = events(new);
        assert_eq!(new_events.len(), 1, "no changed occurrence left");
        assert!(has(new, "DTSTART;TZID=Europe/Berlin:20261013T090000"));
    }

    #[test]
    fn following_counts_the_rest_of_a_count() {
        // 10 runs from 12 Oct; from 15 Oct on: 7 left, the old series keeps 12 to 14 Oct.
        let key = berlin(2026, 10, 15, 8);
        let event = Event { location: Some("Park".into()), ..occurrence(COUNTED, key) };
        let outcome = save(COUNTED, &event, SeriesScope::Following, &writing()).expect("saved");
        let (new, old) = split(&outcome);
        assert!(has(new, "RRULE:FREQ=DAILY;COUNT=7"), "{new}");
        assert!(has(old, "RRULE:FREQ=DAILY;UNTIL=20261015T055959Z"), "{old}");
        assert_eq!(events(old).len(), 2, "the change of 14 Oct stays with the old series");
        assert_eq!(events(new).len(), 1);
        assert_eq!(drops(COUNTED, &event, SeriesScope::Following), 0);
    }

    #[test]
    fn following_from_the_first_occurrence_changes_the_whole_series() {
        let key = berlin(2026, 9, 1, 10);
        let event = Event { summary: "Sync".into(), ..occurrence(EXCEPTIONS, key) };
        let outcome = save(EXCEPTIONS, &event, SeriesScope::Following, &writing()).expect("saved");
        assert!(matches!(outcome.plan, Plan::Replace { .. }));
        assert_eq!(delete(EXCEPTIONS, &event, SeriesScope::Following, now()).map(|o| o.plan), Ok(Plan::Delete));
    }

    #[test]
    fn deleting_following_ends_the_series_before() {
        let key = berlin(2026, 10, 13, 10);
        let outcome = delete(EXCEPTIONS, &occurrence(EXCEPTIONS, key), SeriesScope::Following, now()).expect("ok");
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "RRULE:FREQ=WEEKLY;BYDAY=TU;UNTIL=20261013T075959Z"));
        let keys: Vec<Option<EventTime>> = events(ics).iter().map(|e| e.recurrence_id).collect();
        assert_eq!(keys, [None, Some(berlin(2026, 10, 6, 10))], "the later change is gone with its date");
        keeps_foreign_lines(ics);
        // All-day: the day before.
        let outcome =
            delete(ALL_DAY, &occurrence(ALL_DAY, day(2026, 10, 23)), SeriesScope::Following, now()).expect("ok");
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "RRULE:FREQ=WEEKLY;BYDAY=FR;UNTIL=20261022"));
        assert!(has(ics, "EXDATE;VALUE=DATE:20261016"));
        assert_eq!(events(ics).len(), 1);
    }

    #[test]
    fn across_the_change_to_winter_time_the_wall_clock_stays() {
        // 10:00 in Berlin is 08:00 UTC before 25 Oct and 09:00 UTC after.
        let key = berlin(2026, 11, 6, 10);
        let stored = occurrence(DST, key);
        assert_eq!(stored.start, EventTime::DateTime(Utc.with_ymd_and_hms(2026, 11, 6, 9, 0, 0).single().expect("t")));
        let event = Event { start: berlin(2026, 11, 6, 9), end: Some(berlin(2026, 11, 6, 10)), ..stored };
        let outcome = save(DST, &event, SeriesScope::Series, &writing()).expect("saved");
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "DTSTART;TZID=Europe/Berlin:20261016T090000"), "{ics}");
        assert!(has(ics, "RECURRENCE-ID;TZID=Europe/Berlin:20261030T090000"));
        assert_eq!(events(ics)[1].start, berlin(2026, 10, 30, 12), "moved by itself: stays at 12:00");
        assert!(has(ics, "RRULE:FREQ=WEEKLY;BYDAY=FR;UNTIL=20261127T225959Z"));
        assert_eq!(drops(DST, &event, SeriesScope::Series), 0);
        // Split after the change: the old series ends the second before 10:00 winter time.
        let outcome =
            save(DST, &Event { summary: "Review".into(), ..occurrence(DST, key) }, SeriesScope::Following, &writing())
                .expect("split");
        let (new, old) = split(&outcome);
        assert!(has(old, "RRULE:FREQ=WEEKLY;BYDAY=FR;UNTIL=20261106T085959Z"), "{old}");
        assert!(has(new, "DTSTART;TZID=Europe/Berlin:20261106T100000"));
        assert!(has(new, "RRULE:FREQ=WEEKLY;BYDAY=FR;UNTIL=20261127T225959Z"));
        assert_eq!(events(old).len(), 2, "the change of 30 Oct stays before the split");
    }

    #[test]
    fn a_single_event_with_a_scope_is_written_as_one_and_can_get_a_rule() {
        let single = include_str!("../tests/fixtures/nextcloud-event-berlin.ics");
        let mut event = events(single).remove(0);
        event.summary = "Planungsrunde".into();
        event.repeat = Some(Box::new(Repeat {
            freq: RepeatFreq::Weekly,
            interval: 1,
            weekdays: Vec::new(),
            month_days: Vec::new(),
            months: Vec::new(),
            end: RepeatEnd::Count(4),
            rule: String::new(),
            custom: false,
        }));
        let outcome = save(single, &event, SeriesScope::Series, &writing()).expect("saved");
        assert_eq!((outcome.reset, outcome.key), (0, None));
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "RRULE:FREQ=WEEKLY;COUNT=4"));
        assert!(has(ics, "COLOR:steelblue"));
        assert_eq!(drops(single, &event, SeriesScope::Series), 0);
        assert_eq!(delete(single, &event, SeriesScope::Occurrence, now()).map(|o| o.plan), Ok(Plan::Delete));
    }

    #[test]
    fn a_rule_the_fields_cannot_hold_stays_when_only_the_title_changes() {
        let custom = EXCEPTIONS.replace("RRULE:FREQ=WEEKLY;BYDAY=TU", "RRULE:FREQ=MONTHLY;BYDAY=TU;BYSETPOS=2");
        let key = berlin(2026, 10, 13, 10);
        let stored = occurrence(&custom, key);
        assert!(stored.repeat.as_deref().is_some_and(|r| r.custom));
        let event = Event { summary: "Monthly sync".into(), ..stored };
        let outcome = save(&custom, &event, SeriesScope::Series, &writing()).expect("saved");
        assert!(has(replaced_ics(&outcome), "RRULE:FREQ=MONTHLY;BYDAY=TU;BYSETPOS=2"));
        assert_eq!(drops(&custom, &event, SeriesScope::Series), 0);
    }
}
