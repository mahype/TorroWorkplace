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
//!
//!   A start moved by days while the rule keeps its `BYDAY` or `BYMONTHDAY` (a Tuesday series moved to
//!   Wednesday) is written as it is: RFC 5545 and Nextcloud allow a `DTSTART` that is no date of the
//!   rule, it then counts as one extra occurrence before the rule's dates. The changed occurrences and
//!   excluded dates whose moved date the rule no longer has fall away and are counted ([`drops`],
//!   [`Outcome::reset`]), so the core asks for the right to delete; a client that wants the same days
//!   a day later sends the rule with the new weekday.
//! - **Following:** splits the series as Nextcloud and Google do (never `RANGE=THISANDFUTURE`): the
//!   stored series ends the second before the occurrence (a `COUNT` becomes `UNTIL`), a new object with
//!   a new UID and `RELATED-TO;RELTYPE=SIBLING` starts at it with the rest of the count, and the later
//!   changed occurrences and `EXDATE`s move into it – unless time or rule change, then they are reset
//!   and [`Outcome::reset`] says how many. Extra dates (`RDATE`, written by other programs) from the
//!   occurrence on move into it and along with the series, as with "All"; the series is not split
//!   at an extra date that is no date of its rule. Only what the client changed against the occurrence as
//!   stored (its changed occurrence, if any) goes to the new series; what a changed occurrence changed
//!   itself (moved, cancelled, its own title …) stays its own and stays with it. Time counts as
//!   changed only when the client sends another start or length than the stored occurrence has: a
//!   changed occurrence moved from 10:00 to 14:00 and saved for "following" with a new title keeps
//!   the series at 10:00 and resets nothing; saved at 15:00, the new series starts at 15:00 (measured
//!   from where the rule puts the occurrence). [`drops`] and [`resets`] measure the same way. The first
//!   occurrence counts as the whole series. Deleting only ends the stored series.
//!
//! **Ending a series** takes `repeat: None` *and* `recurring: false` ([`ends_series`]). `repeat: None`
//! alone keeps the rule: an occurrence a client got without its rule (the stored object was not at
//! hand) must not end its series by being sent back.
//!
//! **Zones.** Dates are counted in the zone of the stored `DTSTART`; a timed start without a zone
//! chrono-tz knows (floating, or a name only its `VTIMEZONE` explains) counts in the computer's zone,
//! the zone [`crate::ical`] reads it in. Windows names of the common zones are known
//! ([`crate::ical`]). Lines written here name the zone by its IANA name; the object gets its
//! `VTIMEZONE` when it has none for it.
//!
//! Whatever the suite does not know stays as stored: properties of other programs, alarms, zones. The
//! server keeps expanding the occurrences (decision 1); the `rrule` crate only counts dates here: the
//! rest of a `COUNT`, whether a key belongs to the series, and which events a change drops
//! ([`drops`]), which the core needs to decide whether the save needs the right to delete.

use std::fmt;

use chrono::{DateTime, Datelike, Duration, NaiveDateTime, NaiveTime, Utc};
use chrono_tz::Tz;
use torro_pim::model::{Repeat, RepeatEnd, SeriesScope};
use torro_pim::{Event, EventTime, ProviderError};

use crate::ical::{self, ObjectFacts, Property};
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
    /// A date of the event lies outside the years 1 to 9999, the dates iCalendar can write; counting
    /// with it could leave the dates chrono knows (the review of PR #294).
    OutOfRange,
}

impl SeriesError {
    /// The API's error code: `not-found`, `unsupported` or `error`.
    pub fn code(&self) -> &'static str {
        match self {
            SeriesError::NoOccurrence => "not-found",
            SeriesError::Unsupported(_) => "unsupported",
            SeriesError::OutOfRange => "error",
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
            SeriesError::OutOfRange => f.write_str("the dates of an event lie between the years 1 and 9999"),
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
const UNCOUNTED: &str =
    "the rule of this series cannot be counted up to this event, so it cannot be split: change all events or one";
const EXTRA_DATE: &str = "this event was added to the series apart from its rule, so the series cannot be split here: \
     change all events or this one";

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
        (SeriesScope::Following, Some(main)) => Ok(replaced(&head(&object, main, key, now), Some(main))),
        (SeriesScope::Following, None) => {
            object.retain_vevents(|block| key_of(block).is_none_or(|k| instant(k) < instant(key)));
            Ok(if object.vevents().next().is_none() { gone } else { replaced(&object, None) })
        }
        (SeriesScope::Occurrence, main) => {
            object.retain_vevents(|block| key_of(block) != Some(key));
            if let Some(main) = main {
                let excluded = write::time_line("EXDATE", key, main.zone);
                object.edit_master(|block| touch(&insert(block, vec![excluded]), now));
            }
            Ok(if object.vevents().next().is_none() { gone } else { replaced(&object, main.as_ref()) })
        }
    }
}

/// How many events of the series `stored` a save of `event` with `scope` removes or resets: dates the
/// series no longer has (a shorter `UNTIL` or `COUNT`, another rule; compared as far as dates are
/// counted) and changed occurrences that fall away ([`resets`]). At least 1 when the rule goes, and
/// when a new rule may end sooner than the old one beyond the dates counted ([`ends_sooner`]). Read
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

/// The occurrence with the key `key` of the object `ics` as it was written ([`Plan`]), the way the
/// core caches it after the server expanded it: its uid, start, end and fields, with `recurrence_id`,
/// `repeat`, `time_zone`, `exceptions` and `recurring` as [`ObjectFacts::complete`] puts them; the
/// object's single event for `key: None`. Without `collection`, `href` and `etag`, which the caller
/// knows. `None` when the object holds no such event.
///
/// The answer to a write of a series is built from it when the occurrence is not among those read
/// again (outside the cached window, or reading failed): it is the occurrence written, with its key,
/// never another one, and after a split the uid and rule (the rest of a `COUNT`) of the new series.
pub fn occurrence(ics: &str, key: Option<EventTime>) -> Option<Event> {
    let object = Object::read(ics);
    let mut event = match key {
        Some(key) => match object.exception(key).and_then(|block| event_of(block)) {
            Some(changed) => changed,
            None => object.master().and_then(|block| Main::of(block))?.at(key),
        },
        None => object.master().and_then(|block| event_of(block))?,
    };
    ObjectFacts::read(ics).complete(&mut event);
    Some(event)
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

/// `object` as the stored object becomes ([`zoned`]).
fn replaced(object: &Object, main: Option<&Main>) -> Outcome {
    Outcome { plan: Plan::Replace { ics: zoned(object, main) }, reset: 0, key: None }
}

/// `object` as text ([`Object::text`]), with the `VTIMEZONE` of the series' zone (`main`) added when
/// one of its lines names that zone (`TZID=`) and the object defines none, as RFC 5545 section 3.6.5
/// asks. The lines written here name the zone by its IANA name, also where the stored start names it
/// otherwise (Windows' `W. Europe Standard Time`) or not at all (floating).
fn zoned(object: &Object, main: Option<&Main>) -> String {
    let text = object.text();
    let Some((tz, around)) = main.and_then(|main| main.zone.map(|tz| (tz, main.event.start))) else { return text };
    let name = tz.name();
    let named = ical::unfold(&text)
        .iter()
        .any(|line| ical::parse_line(line).is_some_and(|prop| prop.param("TZID").is_some_and(|zone| zone == name)));
    if !named || object.zones().iter().any(|zone| zone == name) {
        return text;
    }
    let mut object = object.clone();
    object.add_zone(write::vtimezone(tz, instant(around)));
    object.text()
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
    /// The zone its dates are counted and written in ([`count_zone`]); `None` for a day or UTC.
    zone: Option<Tz>,
    /// Its `EXDATE`s.
    excluded: Vec<EventTime>,
    /// Its `RDATE`s.
    extra: Vec<EventTime>,
}

impl Main {
    fn of(block: &[String]) -> Option<Main> {
        let event = event_of(block)?;
        let zone = count_zone(block, &event);
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
        self.occurrences_through(None)
    }

    /// [`Main::occurrences`], counted at least up to `through` (a key the client names, which may lie
    /// beyond the years counted from the start of an old series).
    fn occurrences_through(&self, through: Option<EventTime>) -> Option<Dates> {
        let mut dates = match &self.rule {
            Some(rule) => rule_dates_through(self.event.start, rule, self.zone, through)?,
            None => Dates { list: vec![self.event.start], known_until: None },
        };
        dates.list.extend(self.extra.iter().copied());
        dates.list.sort_by_key(|time| instant(*time));
        dates.list.dedup();
        dates.list.retain(|time| !self.excluded.contains(time));
        Some(dates)
    }
}

/// The zone the dates of the main event `event` (its `block`) are counted in, the zone the reader
/// read its `DTSTART` in: the one its `TZID` names (Windows names of the common zones included,
/// [`ical::zone`]); for a timed start without a zone chrono-tz knows (floating, or a name only the
/// object's `VTIMEZONE` explains) the computer's ([`write::machine_zone`]), where the reader puts it.
/// `None` for a day and for UTC: counting in UTC from a start read in the computer's zone would give
/// keys the server never expands, and every change would look like dropping events.
fn count_zone(block: &[String], event: &Event) -> Option<Tz> {
    if event.all_day() {
        return None;
    }
    if let Some(tz) = event.time_zone.as_deref().and_then(ical::zone) {
        return Some(tz);
    }
    let utc = first(block, "DTSTART").is_some_and(|prop| prop.value.trim().to_ascii_uppercase().ends_with('Z'));
    if utc { None } else { write::machine_zone().filter(|tz| !write::is_utc(*tz)) }
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
///
/// Counted [`HORIZON_DAYS`] from the start, and with `through` at least a day past it (still at most
/// [`LIMIT`] dates), so a key the client names in a series older than that is known (the review of
/// PR #294).
fn rule_dates_through(start: EventTime, rule: &str, zone: Option<Tz>, through: Option<EventTime>) -> Option<Dates> {
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
    let mut horizon = instant(start).checked_add_signed(Duration::days(HORIZON_DAYS))?;
    if let Some(through) = through {
        horizon = horizon.max(instant(through).checked_add_signed(Duration::days(1))?);
    }
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
/// a `COUNT` reduced by the dates before `key`, counted up to `key` (else the rest would come out too
/// large and the new series would run past the old end). `NoOccurrence` when none are left.
fn rest_rule(main: &Main, key: EventTime) -> Result<Option<String>, SeriesError> {
    let Some(rule) = &main.rule else { return Ok(None) };
    let Some(RepeatEnd::Count(count)) = main.repeat().map(|repeat| repeat.end) else { return Ok(Some(rule.clone())) };
    let dates = rule_dates_through(main.event.start, rule, main.zone, Some(key))
        .filter(|dates| dates.knows(key))
        .ok_or(SeriesError::Unsupported(UNCOUNTED))?;
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
    in_range(event)?;
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
    // A key the series does not have would make an extra event on the server; counted up to the key,
    // also in a series older than the years counted from its start.
    if let Some(dates) = held.occurrences_through(Some(key))
        && dates.knows(key)
        && !dates.list.contains(&key)
    {
        return Err(SeriesError::NoOccurrence);
    }
    let before = held.at(key);
    Ok(Found { object, main, before, key: Some(key) })
}

/// Whether the dates the client sends lie in the years 1 to 9999, as iCalendar writes them: start,
/// end, key and the end of the rule. Every count and shift here starts from them and stays within
/// chrono's dates then (the stored dates have four digits too), so a date at chrono's edge is refused
/// instead of panicking: the save answers `error`. [`drops`], which runs before any right is checked,
/// counts such a refused change as dropping (fail closed), so a client without the right to delete
/// gets `forbidden` first.
fn in_range(event: &Event) -> Result<(), SeriesError> {
    let until = event.repeat.as_deref().and_then(|repeat| match repeat.end {
        RepeatEnd::Until(until) => Some(until),
        RepeatEnd::Never | RepeatEnd::Count(_) => None,
    });
    let year = |time: EventTime| match time {
        EventTime::Date(day) => day.year(),
        EventTime::DateTime(at) => at.year(),
    };
    let fine = [Some(event.start), event.end, event.recurrence_id, until]
        .into_iter()
        .flatten()
        .all(|time| (1..=9999).contains(&year(time)));
    if fine { Ok(()) } else { Err(SeriesError::OutOfRange) }
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

/// Whether saving `event` with a scope `series` or `following` ends its series: `repeat: None` together
/// with `recurring: false`. `repeat: None` with `recurring: true` keeps the stored rule, so an
/// occurrence a client got without its rule (outside the cached window, the stored object not at
/// hand) does not end the series by being sent back (finding 3 of the review of #278). The core and
/// the approvals read it from here, so they say the same as the save does.
pub fn ends_series(event: &Event) -> bool {
    event.repeat.is_none() && !event.recurring
}

fn rule_change(main: &Main, event: &Event) -> Rule {
    match event.repeat.as_deref() {
        None if main.is_series() && ends_series(event) => Rule::Remove,
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
    Ok(Outcome { plan: Plan::Replace { ics: zoned(&object, found.main.as_ref()) }, reset: 0, key: Some(key) })
}

fn save_series(found: Found, key: EventTime, event: &Event, writing: &Writing<'_>) -> Result<Outcome, SeriesError> {
    let main = found.main.as_ref().ok_or(SeriesError::Unsupported(NO_MAIN))?;
    let rule = rule_change(main, event);
    let mut object = found.object;
    let edited = edit_series(&mut object, &found.before, key, event, &rule, writing)?;
    Ok(Outcome { plan: Plan::Replace { ics: zoned(&object, Some(main)) }, reset: edited.reset, key: edited.key })
}

fn save_following(found: Found, key: EventTime, event: &Event, writing: &Writing<'_>) -> Result<Outcome, SeriesError> {
    let main = found.main.as_ref().ok_or(SeriesError::Unsupported(NO_MAIN))?;
    let rule = rule_change(main, event);
    let reference = following_reference(main, &found.before, key, event);
    let shift = shift_between(reference.start, event.start, main.zone).ok_or(SeriesError::Unsupported(KIND_CHANGE))?;
    let resized = length(reference.start, reference.end) != length(event.start, event.end);
    let head = head(&found.object, main, key, writing.now);
    let mut tail = tail(&found.object, main, key, writing.new_uid)?;
    let mut reset = 0;
    // Nextcloud's rule (decision 4): later changed occurrences move only while the dates stay. The
    // occurrence's own changed occurrence stays: what it changed itself is its own, not the series'.
    // Excluded dates come back; extra dates (`RDATE`) stay and move with the series as with "All",
    // since dropping them would remove events [`drops`] does not count (the review of PR #294).
    if !shift.is_zero() || resized || rule != Rule::Keep {
        reset = tail.exception_keys().into_iter().filter(|other| *other != key).count() as u32;
        tail.retain_vevents(|block| is_master(block) || key_of(block) == Some(key));
        tail.edit_master(|block| set(block, &["EXDATE"], Vec::new()));
    }
    let edited = edit_series(&mut tail, &reference, key, event, &rule, writing)?;
    let plan = Plan::Split { new: zoned(&tail, Some(main)), old: zoned(&head, Some(main)) };
    Ok(Outcome { plan, reset: reset + edited.reset, key: edited.key })
}

/// What "following" measures a change of the occurrence at `key` against (the second review of #278,
/// findings 2 and 3): the fields of `before`, the occurrence as stored (its changed occurrence, if
/// any), so only what the client changed goes to the new series. The time is that of `before` while
/// the client keeps its start: then the new series keeps the clock of the main event (no shift, no
/// reset), also when `before` was moved by itself. A new start is measured from where the rule puts
/// the occurrence (`main` at `key`), so the new series starts where the client put it. The length is
/// always `before`'s: a length the client did not change changes nothing.
fn following_reference(main: &Main, before: &Event, key: EventTime, event: &Event) -> Event {
    let start = if event.start == before.start { before.start } else { main.at(key).start };
    let end = length(before.start, before.end).map(|length| plus(start, length));
    Event { start, end, ..before.clone() }
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
    check_split(main, key)?;
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

/// Whether `main`'s series can be split at `key`. The new series' rule counts from its start, so a
/// start on an extra date (`RDATE`, written by another program) that is no date of the rule would move
/// the rule there: weekly on Tuesdays at 10:00 would become Thursdays at 15:00 (the review of PR #294).
/// Such a key is refused; so is a key whose place in the rule cannot be told (more than [`LIMIT`]
/// dates before it): split there, the rule would be anchored on a date it may not have. A series
/// without a rule has nothing to move. A rule the counter cannot read at all (one without `FREQ` a
/// server stores) is split as before, refused only at an extra date: what it means is not known
/// here, and its series stays changeable.
fn check_split(main: &Main, key: EventTime) -> Result<(), SeriesError> {
    let Some(rule) = &main.rule else { return Ok(()) };
    let Some(dates) = rule_dates_through(main.event.start, rule, main.zone, Some(key)) else {
        return if main.extra.contains(&key) { Err(SeriesError::Unsupported(EXTRA_DATE)) } else { Ok(()) };
    };
    if !dates.knows(key) {
        return Err(SeriesError::Unsupported(UNCOUNTED));
    }
    if dates.list.contains(&key) { Ok(()) } else { Err(SeriesError::Unsupported(EXTRA_DATE)) }
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
            // The rule as the block will hold it (its `RRULE` line is set below), so writing the
            // event onto the block keeps that text.
            let repeat = match rule {
                Rule::Set(text) => Some(crate::recur::read(text, zone)),
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
            if let Rule::Set(text) = rule {
                lines = set(&lines, &["RRULE"], vec![format!("RRULE:{text}")]);
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
    if following {
        check_split(main, key)?;
    }
    // As the save measures it ([`following_reference`]).
    let reference = if following { following_reference(main, &found.before, key, event) } else { found.before.clone() };
    let shift = shift_between(reference.start, event.start, zone).ok_or(SeriesError::Unsupported(KIND_CHANGE))?;
    let resized = length(reference.start, reference.end) != length(event.start, event.end);
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
    // "Following" compares the series from `key` on as the split's new object holds it before the
    // change (the rule from `key`, as [`tail`] writes it), so dates are counted from the key and not
    // from a start that may lie more than the counted ten years back (the API review of PR #294).
    let old_dates = if following {
        let rest = rest_rule(main, key)?;
        let extra = main.extra.iter().copied().filter(|time| from(*time)).collect();
        let excluded = main.excluded.clone();
        Main { event: Event { start: key, ..main.event.clone() }, rule: rest, zone, excluded, extra }.occurrences()
    } else {
        main.occurrences()
    };
    // Dates are compared only as far as they are counted; a new rule that ends sooner than the old
    // one beyond that drops events nobody counted (the review of PR #294).
    // So does a later start under a fixed `UNTIL`: the last dates move past it.
    let old_end = main.repeat().map(|r| r.end);
    let new_end = match &rule {
        Rule::Set(text) => Some(crate::recur::read(text, zone).end),
        Rule::Keep => old_end,
        Rule::Remove => None,
    };
    let later = shift > Duration::zero() && matches!(new_end, Some(RepeatEnd::Until(_)));
    let cut_short = old_dates.as_ref().is_some_and(|old| old.known_until.is_some())
        && match &rule {
            Rule::Set(_) => new_end.is_some_and(|end| ends_sooner(end, old_end)) || later,
            Rule::Keep => later,
            Rule::Remove => false,
        };
    let mut gone: Vec<EventTime> = match (old_dates, &new) {
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
    if rule == Rule::Remove || cut_short {
        dropped = dropped.max(1);
    }
    Ok(Lost { dropped, reset: reset.len() as u32 })
}

/// Whether a series ending with `new` may end before one ending with `old` (`None`: no rule): an
/// end after a series that never ends, a count (whose last date is not known here) or an `UNTIL`
/// before the old one. A new rule that never ends, or ends later on the same kind of end, does not.
fn ends_sooner(new: RepeatEnd, old: Option<RepeatEnd>) -> bool {
    match (new, old) {
        (RepeatEnd::Never, _) => false,
        (RepeatEnd::Until(new), Some(RepeatEnd::Until(old))) => instant(new) < instant(old),
        (RepeatEnd::Count(new), Some(RepeatEnd::Count(old))) => new < old,
        _ => true,
    }
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
    const RDATES: &str = include_str!("../tests/fixtures/other-series-rdate.ics");
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
        let event = Event { repeat: None, recurring: false, ..occurrence(EXCEPTIONS, key) };
        assert!(ends_series(&event));
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
    fn an_end_beyond_the_counted_dates_still_drops_events() {
        // The review of PR #294: weekly without end, ended in eleven years – beyond the ten counted.
        let key = berlin(2026, 10, 13, 10);
        let stored = occurrence(EXCEPTIONS, key);
        let ending = |end| {
            let mut repeat = stored.repeat.as_deref().cloned().expect("rule");
            repeat.end = end;
            Event { repeat: Some(Box::new(repeat)), ..stored.clone() }
        };
        let far = EventTime::Date(NaiveDate::from_ymd_opt(2037, 12, 31).expect("day"));
        assert_eq!(drops(EXCEPTIONS, &ending(RepeatEnd::Until(far)), SeriesScope::Series), 1);
        assert_eq!(drops(EXCEPTIONS, &ending(RepeatEnd::Count(900)), SeriesScope::Series), 1);
        assert_eq!(drops(EXCEPTIONS, &ending(RepeatEnd::Count(900)), SeriesScope::Following), 1);
        assert_eq!(drops(EXCEPTIONS, &stored, SeriesScope::Series), 0, "the rule unchanged drops nothing");
        assert!(ends_sooner(RepeatEnd::Count(3), Some(RepeatEnd::Never)));
        assert!(!ends_sooner(RepeatEnd::Never, Some(RepeatEnd::Count(3))));
        assert!(!ends_sooner(RepeatEnd::Until(far), Some(RepeatEnd::Until(day(2030, 1, 1)))));
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

    /// The occurrences the main event of `ics` gives, as the counter counts them.
    fn dates_of(ics: &str) -> Vec<EventTime> {
        let object = Object::read(ics);
        let main = object.master().and_then(|block| Main::of(block)).expect("a main event");
        main.occurrences().expect("countable").list
    }

    #[test]
    fn following_in_a_series_older_than_the_counted_years_still_counts_what_it_drops() {
        // The API review of PR #294: Tuesdays and Thursdays since 2014, from 13 Oct 2026 Tuesdays only.
        let old = old_series("FREQ=WEEKLY;BYDAY=TU,TH");
        assert!(!old.contains("RECURRENCE-ID") && old.contains("BEGIN:VTIMEZONE"), "{old}");
        let key = berlin(2026, 10, 13, 8);
        let stored = occurrence(&old, key);
        let mut tuesdays = stored.repeat.as_deref().cloned().expect("rule");
        tuesdays.weekdays.retain(|day| day.day == torro_pim::model::Weekday::Tuesday);
        let event = Event { repeat: Some(Box::new(tuesdays)), ..stored.clone() };
        assert!(drops(&old, &event, SeriesScope::Following) > 0, "the Thursdays from 15 Oct 2026 on");
        let every_other = Repeat { interval: 2, ..stored.repeat.as_deref().cloned().expect("rule") };
        let event = Event { repeat: Some(Box::new(every_other)), ..stored.clone() };
        assert!(drops(&old, &event, SeriesScope::Following) > 0, "every other week");
        let renamed = Event { summary: "Run".into(), ..stored };
        assert_eq!(drops(&old, &renamed, SeriesScope::Following), 0, "a new title drops nothing");
    }

    /// `COUNTED` without its changed occurrence, starting on Tuesday 7 Jan 2014 at 08:00 with `rule`.
    fn old_series(rule: &str) -> String {
        plain_series(rule, "20140107")
    }

    /// `COUNTED` without its changed occurrence, starting on `day` (`YYYYMMDD`) at 08:00 with `rule`.
    fn plain_series(rule: &str, day: &str) -> String {
        let second = COUNTED.match_indices("BEGIN:VEVENT").nth(1).map(|(at, _)| at).expect("a changed occurrence");
        let end = COUNTED[second..].find("END:VEVENT\r\n").or_else(|| COUNTED[second..].find("END:VEVENT\n"));
        let end = second + end.expect("its end") + "END:VEVENT\n".len();
        format!("{}{}", &COUNTED[..second], COUNTED[end..].trim_start_matches('\n'))
            .replace("DTSTART;TZID=Europe/Berlin:20261012T080000", &format!("DTSTART;TZID=Europe/Berlin:{day}T080000"))
            .replace("DTEND;TZID=Europe/Berlin:20261012T083000", &format!("DTEND;TZID=Europe/Berlin:{day}T083000"))
            .replace("RRULE:FREQ=DAILY;COUNT=10", &format!("RRULE:{rule}"))
    }

    #[test]
    fn a_key_beyond_the_counted_years_is_checked_against_the_rule() {
        // The second review of PR #294: Tuesdays since 2014; a Thursday is no event of it, also now.
        let old = old_series("FREQ=WEEKLY;BYDAY=TU");
        let thursday = berlin(2026, 10, 15, 15);
        let event = Event { summary: "Moved".into(), ..occurrence(&old, berlin(2026, 10, 13, 8)) };
        let event = Event { recurrence_id: Some(thursday), start: thursday, end: None, ..event };
        for scope in [SeriesScope::Occurrence, SeriesScope::Following, SeriesScope::Series] {
            assert_eq!(save(&old, &event, scope, &writing()).map(|_| ()), Err(SeriesError::NoOccurrence), "{scope:?}");
        }
        assert_eq!(drops(&old, &event, SeriesScope::Following), 1);
        // A Tuesday now still splits, with the rule anchored on it.
        let tuesday = Event { summary: "Run".into(), ..occurrence(&old, berlin(2026, 10, 13, 8)) };
        let outcome = save(&old, &tuesday, SeriesScope::Following, &writing()).expect("split");
        let (new, _) = split(&outcome);
        assert!(has(new, "DTSTART;TZID=Europe/Berlin:20261013T080000"), "{new}");
    }

    #[test]
    fn the_rest_of_a_count_is_counted_up_to_a_key_beyond_the_counted_years() {
        // 1000 Tuesdays from 7 Jan 2014: 666 lie before 13 Oct 2026, 334 are left.
        let old = old_series("FREQ=WEEKLY;BYDAY=TU;COUNT=1000");
        let event = Event { summary: "Run".into(), ..occurrence(&old, berlin(2026, 10, 13, 8)) };
        let outcome = save(&old, &event, SeriesScope::Following, &writing()).expect("split");
        let (new, _) = split(&outcome);
        assert!(has(new, "RRULE:FREQ=WEEKLY;BYDAY=TU;COUNT=334"), "{new}");
    }

    #[test]
    fn a_later_start_under_an_until_beyond_the_counted_years_drops_the_last_event() {
        // Tuesdays from 2026 until the last one in 2040; an hour later, that last one passes the end.
        let until = plain_series("FREQ=WEEKLY;UNTIL=20401225T070000Z", "20261013");
        let key = berlin(2026, 10, 20, 8);
        let stored = occurrence(&until, key);
        let later = Event { start: berlin(2026, 10, 20, 9), end: Some(berlin(2026, 10, 20, 10)), ..stored.clone() };
        assert_eq!(drops(&until, &later, SeriesScope::Series), 1);
        let earlier = Event { start: berlin(2026, 10, 20, 7), end: Some(berlin(2026, 10, 20, 8)), ..stored };
        assert_eq!(drops(&until, &earlier, SeriesScope::Series), 0, "an earlier start keeps every event");
    }

    #[test]
    fn following_with_a_new_time_keeps_the_extra_dates_of_other_programs() {
        // Weekly on Tuesdays at 10:00 from 1 Sep, with extra dates on Thursday 15 and 29 Oct at 15:00.
        let key = berlin(2026, 10, 13, 10);
        let stored = occurrence(RDATES, key);
        let event = Event { start: berlin(2026, 10, 13, 9), end: Some(berlin(2026, 10, 13, 10)), ..stored };
        assert_eq!(drops(RDATES, &event, SeriesScope::Following), 0, "every event stays, an hour earlier");
        let outcome = save(RDATES, &event, SeriesScope::Following, &writing()).expect("saved");
        let (new, _) = split(&outcome);
        let dates = dates_of(new);
        for extra in [berlin(2026, 10, 15, 14), berlin(2026, 10, 29, 14)] {
            assert!(dates.contains(&extra), "{extra:?} moves with the series: {new}");
        }
        assert!(dates.contains(&berlin(2026, 10, 20, 9)), "the rule at the new time: {new}");
    }

    #[test]
    fn following_is_refused_at_an_extra_date_off_the_rule() {
        // Splitting at Thursday 15 Oct 15:00 would anchor the weekly rule on Thursdays at 15:00.
        let key = berlin(2026, 10, 15, 15);
        let event = Event { summary: "Team meeting, new room".into(), ..occurrence(RDATES, key) };
        let refused = save(RDATES, &event, SeriesScope::Following, &writing());
        assert!(matches!(refused, Err(SeriesError::Unsupported(_))), "{refused:?}");
        assert_eq!(drops(RDATES, &event, SeriesScope::Following), 1, "a refused change counts as dropping");
        // This event alone and all events still work; deleting the following ones ends the series.
        assert!(save(RDATES, &event, SeriesScope::Occurrence, &writing()).is_ok());
        assert!(save(RDATES, &event, SeriesScope::Series, &writing()).is_ok());
        let ended = delete(RDATES, &event, SeriesScope::Following, now()).expect("deleted");
        assert!(dates_of(replaced_ics(&ended)).iter().all(|date| instant(*date) < instant(key)));
    }

    #[test]
    fn dates_beyond_what_icalendar_writes_are_an_error_not_a_panic() {
        // The review of PR #294: counting from a date at chrono's edge would overflow.
        let key = berlin(2026, 10, 13, 10);
        let edge = EventTime::Date(NaiveDate::MAX);
        let far = EventTime::DateTime(Utc.with_ymd_and_hms(10000, 1, 1, 0, 0, 0).single().expect("far"));
        for (start, end) in [(edge, None), (far, None), (berlin(2026, 10, 13, 9), Some(far))] {
            let event = Event { start, end, ..occurrence(EXCEPTIONS, key) };
            for scope in [SeriesScope::Occurrence, SeriesScope::Following, SeriesScope::Series] {
                assert_eq!(save(EXCEPTIONS, &event, scope, &writing()).map(|_| ()), Err(SeriesError::OutOfRange));
            }
            assert_eq!(drops(EXCEPTIONS, &event, SeriesScope::Series), 1);
            assert_eq!(SeriesError::OutOfRange.code(), "error");
        }
        let until = Repeat {
            end: RepeatEnd::Until(far),
            ..occurrence(EXCEPTIONS, key).repeat.as_deref().cloned().expect("rule")
        };
        let event = Event { repeat: Some(Box::new(until)), ..occurrence(EXCEPTIONS, key) };
        assert_eq!(drops(EXCEPTIONS, &event, SeriesScope::Series), 1);
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

    // Findings of the review of #278.

    /// Finding 3: an occurrence sent back without its rule (the client never got it) keeps the series.
    #[test]
    fn no_rule_without_recurring_false_keeps_the_series() {
        let key = berlin(2026, 10, 13, 10);
        let event = Event { repeat: None, summary: "Product sync".into(), ..occurrence(EXCEPTIONS, key) };
        assert!(event.recurring && !ends_series(&event));
        for scope in [SeriesScope::Series, SeriesScope::Following] {
            assert_eq!(drops(EXCEPTIONS, &event, scope), 0, "{scope:?}");
            assert_eq!(resets(EXCEPTIONS, &event, scope), 0, "{scope:?}");
        }
        let outcome = save(EXCEPTIONS, &event, SeriesScope::Series, &writing()).expect("saved");
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "RRULE:FREQ=WEEKLY;BYDAY=TU"), "{ics}");
        assert_eq!(events(ics).len(), 3, "the changed occurrences stay");
        let following = save(EXCEPTIONS, &event, SeriesScope::Following, &writing()).expect("split");
        let (new, _) = split(&following);
        assert!(has(new, "RRULE:FREQ=WEEKLY;BYDAY=TU"), "{new}");
    }

    /// Finding 1: the text of a rule never reaches the object unless it is the stored one or, for a
    /// new series, a valid one-line rule.
    #[test]
    fn a_rule_text_from_the_client_cannot_add_lines() {
        let key = berlin(2026, 10, 13, 10);
        let smuggled = "FREQ=WEEKLY;BYDAY=TU\r\nATTENDEE:mailto:eve@example.org\r\nEND:VEVENT";
        let stored = occurrence(EXCEPTIONS, key);
        let repeat =
            |custom: bool| Repeat { rule: smuggled.into(), custom, ..stored.repeat.as_deref().cloned().expect("rule") };
        for custom in [true, false] {
            let event = Event { repeat: Some(Box::new(repeat(custom))), ..stored.clone() };
            for scope in [SeriesScope::Series, SeriesScope::Following, SeriesScope::Occurrence] {
                let outcome = save(EXCEPTIONS, &event, scope, &writing()).expect("saved");
                let texts: Vec<&str> = match &outcome.plan {
                    Plan::Replace { ics } => vec![ics],
                    Plan::Split { new, old } => vec![new, old],
                    Plan::Delete => Vec::new(),
                };
                for ics in texts {
                    assert!(!ics.contains("eve@example.org") && !ics.contains("ATTENDEE"), "{scope:?}: {ics}");
                    assert_eq!(count(ics, "END:VEVENT"), count(ics, "BEGIN:VEVENT"), "{ics}");
                    let rules: Vec<String> =
                        lines(ics).into_iter().filter(|l| l.starts_with("RRULE:FREQ=WEEKLY")).collect();
                    assert!(rules.iter().all(|l| l.starts_with("RRULE:FREQ=WEEKLY;BYDAY=TU")), "{rules:?}");
                }
            }
            assert_eq!(drops(EXCEPTIONS, &event, SeriesScope::Series), 0, "the stored rule stays");
        }
        // A new series: a custom text is written only when it is one valid line.
        let single = include_str!("../tests/fixtures/nextcloud-event-berlin.ics");
        let mut event = events(single).remove(0);
        event.repeat = Some(Box::new(Repeat { rule: smuggled.into(), custom: true, ..repeat(true) }));
        let ics = write::event_onto(single, &event, now(), Some(Tz::Europe__Berlin));
        assert!(!ics.contains("ATTENDEE") && count(&ics, "RRULE:FREQ=WEEKLY") == 0, "{ics}");
        event.repeat = Some(Box::new(Repeat { rule: "FREQ=MONTHLY;BYDAY=TU;BYSETPOS=2".into(), ..repeat(true) }));
        let ics = write::event_onto(single, &event, now(), Some(Tz::Europe__Berlin));
        assert!(has(&ics, "RRULE:FREQ=MONTHLY;BYDAY=TU;BYSETPOS=2"), "{ics}");
    }

    /// Finding 4: "following" on a changed occurrence moved from 10:00 to 14:00, saved at 15:00,
    /// starts the new series at 15:00; the fields the occurrence changed itself stay its own (the
    /// second review of #278, findings 2 and 3).
    #[test]
    fn following_on_a_moved_occurrence_starts_where_the_client_put_it() {
        let moved = EXCEPTIONS
            .replace("DTSTART;TZID=Europe/Berlin:20261027T100000", "DTSTART;TZID=Europe/Berlin:20261027T140000")
            .replace("DTEND;TZID=Europe/Berlin:20261027T110000", "DTEND;TZID=Europe/Berlin:20261027T150000");
        let key = berlin(2026, 10, 27, 10);
        let stored = occurrence(&moved, key);
        assert_eq!(stored.start, berlin(2026, 10, 27, 14));
        let event = Event { start: berlin(2026, 10, 27, 15), end: Some(berlin(2026, 10, 27, 16)), ..stored.clone() };
        let outcome = save(&moved, &event, SeriesScope::Following, &writing()).expect("split");
        assert_eq!((outcome.reset, outcome.key), (0, Some(berlin(2026, 10, 27, 15))));
        let (new, old) = split(&outcome);
        assert!(has(new, "DTSTART;TZID=Europe/Berlin:20261027T150000"), "{new}");
        assert!(has(new, "DTEND;TZID=Europe/Berlin:20261027T160000"), "{new}");
        let new_events = events(new);
        assert_eq!(new_events.len(), 2, "the main event and the occurrence's own change");
        assert_eq!(new_events[0].summary, "Weekly product sync", "its own title is not the series'");
        assert_eq!(new_events[0].description.as_deref(), Some("Roadmap, blockers, next steps."));
        let own = &new_events[1];
        assert_eq!((own.recurrence_id, own.start), (Some(berlin(2026, 10, 27, 15)), berlin(2026, 10, 27, 15)));
        assert_eq!(own.summary, "Weekly product sync (Q4 review)", "its own title stays with it");
        assert_eq!(own.description.as_deref(), Some("Q4 numbers first."), "its own notes stay with it");
        // 27 Oct 10:00 in Berlin is 09:00 UTC (winter time).
        assert!(has(old, "RRULE:FREQ=WEEKLY;BYDAY=TU;UNTIL=20261027T085959Z"), "{old}");
        assert_eq!(drops(&moved, &event, SeriesScope::Following), 0);
        assert_eq!(resets(&moved, &event, SeriesScope::Following), 0);
    }

    /// The second review of #278, findings 2 and 3: "following" with only a new title on a changed
    /// occurrence moved from 10:00 to 11:00 and cancelled keeps the new series at 10:00, resets
    /// nothing, keeps the later changes and excluded dates (no right to delete needed), and carries
    /// neither the move nor the cancellation to the other events.
    #[test]
    fn following_with_a_new_title_on_a_moved_occurrence_keeps_the_clock_of_the_series() {
        let cancelled = EXCEPTIONS
            .replace(
                "DTEND;TZID=Europe/Berlin:20261006T120000\nSTATUS:CONFIRMED",
                "DTEND;TZID=Europe/Berlin:20261006T120000\nSTATUS:CANCELLED",
            )
            .replace(
                "EXDATE;TZID=Europe/Berlin:20260922T100000",
                "EXDATE;TZID=Europe/Berlin:20260922T100000\nEXDATE;TZID=Europe/Berlin:20261020T100000",
            );
        let key = berlin(2026, 10, 6, 10);
        let stored = occurrence(&cancelled, key);
        assert_eq!(stored.start, berlin(2026, 10, 6, 11), "moved to 11:00");
        assert_eq!(stored.status, Some(torro_pim::model::EventStatus::Cancelled), "and cancelled");
        let renamed = Event { summary: "Product sync".into(), ..stored };
        assert_eq!(resets(&cancelled, &renamed, SeriesScope::Following), 0);
        assert_eq!(drops(&cancelled, &renamed, SeriesScope::Following), 0, "no right to delete needed");
        let outcome = save(&cancelled, &renamed, SeriesScope::Following, &writing()).expect("split");
        assert_eq!((outcome.reset, outcome.key), (0, Some(key)));
        let (new, old) = split(&outcome);
        assert!(has(new, "DTSTART;TZID=Europe/Berlin:20261006T100000"), "the clock of the series: {new}");
        assert!(has(new, "EXDATE;TZID=Europe/Berlin:20261020T100000"), "the cancelled 20 Oct stays cancelled");
        assert!(has(old, "RRULE:FREQ=WEEKLY;BYDAY=TU;UNTIL=20261006T075959Z"), "{old}");
        let new_events = events(new);
        assert_eq!(new_events.len(), 3, "the main event, this occurrence's own change and that of 27 Oct");
        let main = &new_events[0];
        assert_eq!(main.summary, "Product sync", "the new title goes to the series");
        assert_eq!(main.status, Some(torro_pim::model::EventStatus::Confirmed), "the cancellation does not");
        let own = new_events.iter().find(|e| e.recurrence_id == Some(key)).expect("its own change stays");
        assert_eq!((own.start, own.summary.as_str()), (berlin(2026, 10, 6, 11), "Product sync"));
        assert_eq!(own.status, Some(torro_pim::model::EventStatus::Cancelled), "still cancelled");
        let later = new_events.iter().find(|e| e.recurrence_id == Some(berlin(2026, 10, 27, 10))).expect("27 Oct");
        assert_eq!(later.summary, "Weekly product sync (Q4 review)", "its own title stays");
        keeps_foreign_lines(new);
    }

    /// Finding 5: a series moved by a day without a new rule keeps its `BYDAY`; what falls away is
    /// counted. With the rule moved along, nothing is lost.
    #[test]
    fn moving_a_series_by_a_day_counts_what_the_old_weekday_loses() {
        let key = berlin(2026, 10, 13, 10);
        let stored = occurrence(EXCEPTIONS, key);
        let wednesday = Event { start: berlin(2026, 10, 14, 10), end: Some(berlin(2026, 10, 14, 11)), ..stored };
        assert_eq!(
            resets(EXCEPTIONS, &wednesday, SeriesScope::Series),
            2,
            "6 and 27 Oct are no Tuesday dates any more"
        );
        assert!(drops(EXCEPTIONS, &wednesday, SeriesScope::Series) > 2);
        let outcome = save(EXCEPTIONS, &wednesday, SeriesScope::Series, &writing()).expect("saved");
        assert_eq!(outcome.reset, 2);
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "DTSTART;TZID=Europe/Berlin:20260902T100000"), "{ics}");
        assert!(has(ics, "RRULE:FREQ=WEEKLY;BYDAY=TU"), "RFC 5545 allows a start off the rule: {ics}");
        assert_eq!(events(ics).len(), 1);

        let mut repeat = wednesday.repeat.as_deref().cloned().expect("rule");
        repeat.weekdays = vec![torro_pim::model::RepeatDay { day: torro_pim::model::Weekday::Wednesday, nth: None }];
        let along = Event { repeat: Some(Box::new(repeat)), ..wednesday };
        assert_eq!(
            (drops(EXCEPTIONS, &along, SeriesScope::Series), resets(EXCEPTIONS, &along, SeriesScope::Series)),
            (0, 0)
        );
        let outcome = save(EXCEPTIONS, &along, SeriesScope::Series, &writing()).expect("saved");
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "RRULE:FREQ=WEEKLY;BYDAY=WE"), "{ics}");
        assert!(has(ics, "RECURRENCE-ID;TZID=Europe/Berlin:20261028T100000"), "{ics}");
        assert!(has(ics, "EXDATE;TZID=Europe/Berlin:20260923T100000"), "{ics}");
        assert_eq!(outcome.reset, 0);
    }

    /// Finding 6: a series from Outlook names its zone the Windows way; its dates are counted there.
    #[test]
    fn a_windows_zone_counts_like_its_iana_zone() {
        const OUTLOOK: &str = include_str!("../tests/fixtures/outlook-series-windows-zone.ics");
        let key = berlin(2026, 10, 13, 10);
        let stored = occurrence(OUTLOOK, key);
        assert_eq!(stored.time_zone.as_deref(), Some("Europe/Berlin"));
        let renamed = Event { summary: "Jour fixe (Q4)".into(), ..stored.clone() };
        assert_eq!(drops(OUTLOOK, &renamed, SeriesScope::Series), 0, "a new title drops nothing");
        let outcome =
            save(OUTLOOK, &renamed, SeriesScope::Occurrence, &writing()).expect("its key is one of the series");
        let ics = replaced_ics(&outcome);
        assert!(has(ics, "RECURRENCE-ID;TZID=Europe/Berlin:20261013T100000"), "{ics}");
        assert_eq!(count(ics, "BEGIN:VTIMEZONE"), 2, "Europe/Berlin is defined for the new line: {ics}");
        assert!(has(ics, "X-MICROSOFT-CDO-BUSYSTATUS:BUSY"));
        // Excluded on 20 Oct, a Tuesday after the change of the clock on 25 Oct still counts at 10:00.
        let excluded = Event { recurrence_id: Some(berlin(2026, 10, 20, 10)), ..stored.clone() };
        assert_eq!(save(OUTLOOK, &excluded, SeriesScope::Occurrence, &writing()), Err(SeriesError::NoOccurrence));
        let winter = occurrence(OUTLOOK, berlin(2026, 11, 3, 10));
        let deleted = delete(OUTLOOK, &winter, SeriesScope::Occurrence, now()).expect("deleted");
        assert!(has(replaced_ics(&deleted), "EXDATE;TZID=Europe/Berlin:20261103T100000"));
        let moved = Event { start: berlin(2026, 10, 13, 11), end: Some(berlin(2026, 10, 13, 12)), ..stored };
        assert_eq!(drops(OUTLOOK, &moved, SeriesScope::Series), 0, "moving an hour drops nothing");
    }

    /// A floating start counts in the computer's zone, where the reader read it.
    #[test]
    fn a_floating_start_counts_in_the_computer_s_zone() {
        let floating = DST.replace("DTSTART;TZID=Europe/Berlin:", "DTSTART:");
        let main = Object::read(&floating).master().and_then(|b| Main::of(b)).expect("main");
        assert_eq!(main.zone, write::machine_zone().filter(|tz| !write::is_utc(*tz)));
        let utc = Object::read(&floating.replace("DTSTART:20261016T100000", "DTSTART:20261016T080000Z"));
        assert_eq!(utc.master().and_then(|b| Main::of(b)).and_then(|m| m.zone), None, "UTC stays UTC");
    }

    /// Finding 2: the answer of a write is the occurrence written, from the object sent.
    #[test]
    fn the_occurrence_written_comes_from_the_object_sent() {
        let key = berlin(2026, 10, 15, 8);
        let event = Event { location: Some("Park".into()), ..occurrence(COUNTED, key) };
        let outcome = save(COUNTED, &event, SeriesScope::Following, &writing()).expect("split");
        let (new, old) = split(&outcome);
        let written = super::occurrence(new, outcome.key).expect("the occurrence");
        assert_eq!((written.uid.as_str(), written.recurrence_id, written.start), (NEW_UID, Some(key), key));
        assert_eq!(written.location.as_deref(), Some("Park"));
        assert_eq!(
            written.repeat.as_deref().map(|r| (r.rule.as_str(), r.end)),
            Some(("FREQ=DAILY;COUNT=7", RepeatEnd::Count(7)))
        );
        assert!(written.recurring && written.time_zone.as_deref() == Some("Europe/Berlin"));
        let changed = super::occurrence(old, Some(berlin(2026, 10, 14, 8))).expect("a changed one of the old series");
        assert_eq!(changed.exceptions, [berlin(2026, 10, 14, 8)]);
        let single = include_str!("../tests/fixtures/nextcloud-event-berlin.ics");
        let one = super::occurrence(single, None).expect("the single event");
        assert_eq!((one.recurrence_id, one.repeat.is_none(), one.recurring), (None, true, false));
    }
}
