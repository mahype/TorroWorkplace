//! Just enough iCalendar (RFC 5545) to read events (VEVENT) and tasks (VTODO).
//!
//! The server expands recurring events for us (`<C:expand>` in the calendar query), so
//! every occurrence arrives as its own VEVENT with times in UTC. A stored object read as it is
//! (an update writes onto it, an invitation came by mail) keeps its `TZID`s: an IANA name
//! (`Europe/Berlin`, also behind a vendor prefix like `/mozilla.org/20050126_1/Europe/Berlin`) is
//! converted with `chrono-tz` and kept as the event's `time_zone` (task TE.2); any other name
//! (Windows' `W. Europe Standard Time`) and floating times are read as the computer's local time.
//!
//! Cancelled events (`STATUS:CANCELLED`) are read with their status (task TE.2): the calendar
//! shows them struck through, and [`Event::busy`] and the reminders leave them out. Cancelled
//! tasks are still skipped. Organizer and attendees are read for display only; nothing writes them.
//!
//! An event's reminders (task ER1.1, `docs/erinnerungen.md`) are its `VALARM` components with
//! `ACTION:DISPLAY` or `AUDIO` ([`alarm`]). Mail alarms, `ACTION:NONE` (Apple's placeholder) and
//! alarms whose `TRIGGER` cannot be read are not the model's: they are left out, not reported.
//!
//! What the server's expansion loses (task TE.3, `docs/termin-editor.md` decisions 1 and 2) comes
//! from the stored object of each `href` ([`ObjectFacts`]): the rule of the series, the zones of
//! its `DTSTART`s and which occurrences were changed. Nextcloud (Sabre) gives every occurrence it
//! expands a `RECURRENCE-ID` (its start, in UTC; a changed one keeps its own) and turns every time
//! into UTC, dropping the `VTIMEZONE`s (`Sabre\VObject\Component\VCalendar::expand`,
//! `Recur\EventIterator::getEventObject`); the fixture `nextcloud-expand-series.xml` shows it.
//! For a server that leaves the key out, [`ObjectFacts::complete`] derives it.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use torro_pim::model::{Attendee, AttendeeRole, EventStatus, Geo, Link, PartStat, Repeat, RepeatEnd, RepeatFreq};
use torro_pim::{Alarm, AlarmAnchor, AlarmStyle, AlarmTrigger, Event, EventTime, Todo, Transparency};

/// One content line: `NAME;PARAM=VALUE:value`.
pub(crate) struct Property<'a> {
    pub(crate) name: &'a str,
    pub(crate) params: Vec<(&'a str, &'a str)>,
    pub(crate) value: &'a str,
}

impl Property<'_> {
    pub(crate) fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| *value)
    }
}

/// The events in one iCalendar object (one `calendar-data`), cancelled ones included (with
/// `status: Cancelled`); a VEVENT without a readable `DTSTART` is left out.
pub fn parse_events(ics: &str, collection: &str) -> Vec<Event> {
    let mut events = Vec::new();
    for_each_component(ics, "VEVENT", |props, alarms| events.extend(to_event(props, alarms, collection)));
    events
}

/// The tasks in one iCalendar object (one `calendar-data`), skipping cancelled ones.
pub fn parse_todos(ics: &str, collection: &str) -> Vec<Todo> {
    let mut todos = Vec::new();
    for_each_component(ics, "VTODO", |props, _| todos.extend(to_todo(props, collection)));
    todos
}

/// The iCalendar object holds a series: an event or task with a rule (`RRULE`), extra dates
/// (`RDATE`) or a changed occurrence (`RECURRENCE-ID`). Writing such an object back from one
/// event or task would drop the rule and every exception, so the core refuses it.
pub fn is_series(ics: &str) -> bool {
    let mut series = false;
    for component in ["VEVENT", "VTODO"] {
        for_each_component(ics, component, |props, _| {
            series |= ["RRULE", "RDATE", "RECURRENCE-ID"].iter().any(|name| find(props, name).is_some());
        });
    }
    series
}

/// What the stored object of one `href` (read without expansion, `CalDavClient::objects`) says
/// about the events the server expanded from it (task TE.3): the rule and zone of the series' main
/// event and its changed occurrences. Read once per object with [`ObjectFacts::read`], then put on
/// every occurrence with [`ObjectFacts::complete`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjectFacts {
    /// The rule of the main event (the VEVENT without `RECURRENCE-ID`): its `RRULE` read with
    /// [`crate::recur::read`] in the zone of its `DTSTART`, marked custom when the event also has
    /// extra dates (`RDATE`), an `EXRULE` or more than one `RRULE`. Extra dates without a rule give
    /// a custom rule with empty text (its `freq` then means nothing). `None` for a single event and
    /// for an object holding only changed occurrences.
    pub repeat: Option<Repeat>,
    /// The IANA zone of the main event's `DTSTART` (as [`Event::time_zone`]); `None` for an all-day
    /// or UTC start, a zone chrono-tz does not know, or no main event.
    pub zone: Option<String>,
    /// The changed occurrences, in the stored order.
    pub exceptions: Vec<Exception>,
}

/// One changed occurrence in a stored object (a VEVENT with `RECURRENCE-ID`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exception {
    /// Which occurrence it replaces (`RECURRENCE-ID`), as an instant or a day.
    pub recurrence_id: EventTime,
    /// Where it starts now (`DTSTART`).
    pub start: EventTime,
    /// The IANA zone of its `DTSTART`, as [`ObjectFacts::zone`].
    pub zone: Option<String>,
}

impl ObjectFacts {
    /// The facts of the stored iCalendar object `ics`. Only VEVENTs count; one without a readable
    /// `DTSTART`, a changed occurrence whose `RECURRENCE-ID` cannot be read and every main event
    /// after the first are left out.
    pub fn read(ics: &str) -> ObjectFacts {
        let mut facts = ObjectFacts::default();
        let mut has_main = false;
        for_each_component(ics, "VEVENT", |props, _| {
            let Some(dtstart) = find(props, "DTSTART") else { return };
            let Some(start) = parse_time(dtstart) else { return };
            let tz = matches!(start, EventTime::DateTime(_)).then(|| dtstart.param("TZID").and_then(zone)).flatten();
            let zone = tz.map(|tz| tz.name().to_string());
            match find(props, "RECURRENCE-ID") {
                Some(key) => {
                    if let Some(recurrence_id) = parse_time(key) {
                        facts.exceptions.push(Exception { recurrence_id, start, zone });
                    }
                }
                None if !has_main => {
                    has_main = true;
                    facts.zone = zone;
                    facts.repeat = main_rule(props, tz);
                }
                None => {}
            }
        });
        facts
    }

    /// Puts what the server's expansion lost on `event`, an occurrence of this object (or the
    /// object's single event):
    ///
    /// - `recurrence_id`, when the server left it out and the object is a series: the changed
    ///   occurrence that starts where `event` starts, else `event.start` (an occurrence the rule
    ///   made starts at its key).
    /// - `time_zone`: the zone of the changed occurrence with that key, else of the main event;
    ///   left as it is when the object names none.
    /// - `repeat`: the rule of the series, the same on every occurrence; and `recurring` for an
    ///   occurrence of a series, so it stays true exactly when `recurrence_id` or `repeat` is set.
    pub fn complete(&self, event: &mut Event) {
        let series = self.repeat.is_some() || !self.exceptions.is_empty();
        if series && event.recurrence_id.is_none() {
            let changed = self.exceptions.iter().find(|exception| exception.start == event.start);
            event.recurrence_id = Some(changed.map_or(event.start, |exception| exception.recurrence_id));
        }
        let changed =
            event.recurrence_id.and_then(|key| self.exceptions.iter().find(|exception| exception.recurrence_id == key));
        let zone = changed.map_or(&self.zone, |exception| &exception.zone);
        if let Some(zone) = zone.as_ref().filter(|_| !event.all_day()) {
            event.time_zone = Some(zone.clone());
        }
        event.repeat = self.repeat.clone().map(Box::new);
        event.recurring |= event.recurrence_id.is_some() || event.repeat.is_some();
    }
}

/// The rule of a main event with the properties `props` and the zone `tz` of its `DTSTART`, as
/// [`ObjectFacts::repeat`] describes it.
fn main_rule(props: &[Property<'_>], tz: Option<Tz>) -> Option<Repeat> {
    let count = |name: &str| props.iter().filter(|p| p.name.eq_ignore_ascii_case(name)).count();
    let first = find(props, "RRULE");
    let more = count("RRULE") > 1 || count("RDATE") > 0 || count("EXRULE") > 0;
    let mut repeat = match first {
        Some(rule) => crate::recur::read(rule.value, tz),
        None if more => Repeat {
            freq: RepeatFreq::Daily,
            interval: 1,
            weekdays: Vec::new(),
            month_days: Vec::new(),
            months: Vec::new(),
            end: RepeatEnd::Never,
            rule: String::new(),
            custom: true,
        },
        None => return None,
    };
    repeat.custom |= more;
    Some(repeat)
}

/// Calls `visit` with the own properties of every top-level `component` in `ics` and the
/// properties of each `VALARM` directly inside it. Properties of other nested components are
/// left out.
fn for_each_component(ics: &str, component: &str, mut visit: impl FnMut(&[Property<'_>], &[Vec<Property<'_>>])) {
    let lines = unfold(ics);
    let mut current: Option<(Vec<Property<'_>>, Vec<Vec<Property<'_>>>)> = None;
    // The VALARM being read, while `depth` is 1 inside one.
    let mut alarm: Option<Vec<Property<'_>>> = None;
    let mut depth = 0usize;
    for line in &lines {
        let Some(prop) = parse_line(line) else { continue };
        match (prop.name, current.as_mut()) {
            ("BEGIN", None) if prop.value.eq_ignore_ascii_case(component) => current = Some((Vec::new(), Vec::new())),
            ("BEGIN", Some(_)) => {
                depth += 1;
                if depth == 1 && prop.value.trim().eq_ignore_ascii_case("VALARM") {
                    alarm = Some(Vec::new());
                }
            }
            ("END", Some((_, alarms))) if depth > 0 => {
                if depth == 1
                    && let Some(done) = alarm.take()
                {
                    alarms.push(done);
                }
                depth -= 1;
            }
            ("END", Some(_)) => {
                if let Some((props, alarms)) = current.take() {
                    visit(&props, &alarms);
                }
            }
            (_, Some((props, _))) if depth == 0 => props.push(prop),
            (_, Some(_)) if depth == 1 => {
                if let Some(alarm) = alarm.as_mut() {
                    alarm.push(prop);
                }
            }
            _ => {}
        }
    }
}

/// The first property called `name`.
fn find<'p, 'a>(props: &'p [Property<'a>], name: &str) -> Option<&'p Property<'a>> {
    props.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// The property links to the parent task: `RELATED-TO` with `RELTYPE=PARENT` or without
/// `RELTYPE`, which means PARENT (RFC 5545 §3.2.15). Other kinds (`CHILD`, `SIBLING`, `X-…`)
/// are not the model's.
pub(crate) fn is_parent_link(prop: &Property<'_>) -> bool {
    prop.name.eq_ignore_ascii_case("RELATED-TO")
        && prop.param("RELTYPE").is_none_or(|t| t.eq_ignore_ascii_case("PARENT"))
}

/// The task one VTODO's own properties describe; `None` for a cancelled one.
pub(crate) fn to_todo(props: &[Property<'_>], collection: &str) -> Option<Todo> {
    let get = |name: &str| find(props, name);
    // Cancelled tasks are skipped, like cancelled events.
    if get("STATUS").is_some_and(|p| p.value.eq_ignore_ascii_case("CANCELLED")) {
        return None;
    }
    let parent = props.iter().find(|p| is_parent_link(p)).map(|p| p.value.trim().to_string()).filter(|s| !s.is_empty());
    let completed =
        get("COMPLETED").is_some() || get("STATUS").is_some_and(|p| p.value.eq_ignore_ascii_case("COMPLETED"));
    Some(Todo {
        uid: get("UID").map(|p| p.value.to_string()).unwrap_or_default(),
        collection: collection.to_string(),
        href: None,
        etag: None,
        summary: get("SUMMARY").map(|p| unescape(p.value)).unwrap_or_default(),
        due: get("DUE").and_then(parse_time),
        // 0 means undefined, as does a missing property; 1 is the highest priority.
        priority: get("PRIORITY").and_then(|p| p.value.trim().parse::<u8>().ok()).filter(|p| (1..=9).contains(p)),
        completed,
        parent,
        description: get("DESCRIPTION").map(|p| unescape(p.value)).filter(|s| !s.is_empty()),
        url: get("URL").map(|p| p.value.trim().to_string()).filter(|s| !s.is_empty()),
        categories: categories(props),
        sort_order: get("X-APPLE-SORT-ORDER").and_then(|p| p.value.trim().parse().ok()),
        link: get("X-TORRO-LINK")
            .map(|p| unescape(p.value).trim().to_string())
            .filter(|id| !id.is_empty())
            .map(Link::new),
    })
}

/// The tags of all `CATEGORIES` properties, in order: each holds a comma-separated list, and
/// a comma escaped as `\,` belongs to the tag. Empty tags are dropped.
fn categories(props: &[Property<'_>]) -> Vec<String> {
    props
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case("CATEGORIES"))
        .flat_map(|p| split_list(p.value))
        .map(|tag| unescape(tag).trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect()
}

/// A list value split at the commas that are not escaped; the parts stay escaped.
fn split_list(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (i, c) in value.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            ',' => {
                parts.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}

/// The event one VEVENT's own properties and its `VALARM`s describe; `None` for one without a
/// readable `DTSTART`. A cancelled event is read like any other, with `status: Cancelled`.
pub(crate) fn to_event(props: &[Property<'_>], alarms: &[Vec<Property<'_>>], collection: &str) -> Option<Event> {
    let get = |name: &str| find(props, name);
    let dtstart = get("DTSTART")?;
    let start = parse_time(dtstart)?;
    let end = match get("DTEND").and_then(parse_time) {
        Some(end) => Some(end),
        None => get("DURATION").and_then(|p| parse_duration(p.value)).map(|d| add(start, d)),
    };
    Some(Event {
        uid: get("UID").map(|p| p.value.to_string()).unwrap_or_default(),
        collection: collection.to_string(),
        href: None,
        etag: None,
        recurring: get("RECURRENCE-ID").is_some() || get("RRULE").is_some() || get("RDATE").is_some(),
        summary: get("SUMMARY").map(|p| unescape(p.value)).unwrap_or_default(),
        start,
        end,
        location: get("LOCATION").map(|p| unescape(p.value)).filter(|s| !s.is_empty()),
        description: get("DESCRIPTION").map(|p| unescape(p.value)).filter(|s| !s.is_empty()),
        transparency: transparency(get("TRANSP")),
        alarms: alarms.iter().filter_map(|props| alarm(props)).collect(),
        recurrence_id: get("RECURRENCE-ID").and_then(parse_time),
        // An occurrence the server expanded has lost the rule: the core adds it from the stored
        // object ([`ObjectFacts::complete`], task TE.3).
        repeat: None,
        time_zone: matches!(start, EventTime::DateTime(_))
            .then(|| dtstart.param("TZID").and_then(zone))
            .flatten()
            .map(|tz| tz.name().to_string()),
        url: get("URL").map(|p| p.value.trim().to_string()).filter(|s| !s.is_empty()),
        status: get("STATUS").and_then(|p| status(p.value)),
        geo: geo(props),
        organizer: get("ORGANIZER").map(|p| Box::new(person(p))),
        attendees: props.iter().filter(|p| p.name.eq_ignore_ascii_case("ATTENDEE")).map(person).collect(),
    })
}

/// The IANA zone a `TZID` names, also behind a vendor prefix (`/mozilla.org/20050126_1/Europe/Berlin`,
/// as older Thunderbird and Citadel write it); `None` for any other name (Windows' `W. Europe
/// Standard Time`), which the object's own `VTIMEZONE` would have to explain.
pub(crate) fn zone(tzid: &str) -> Option<Tz> {
    let tzid = tzid.trim();
    let suffixes = tzid.match_indices('/').map(|(i, _)| &tzid[i + 1..]);
    std::iter::once(tzid).chain(suffixes).find_map(|name| name.parse::<Tz>().ok())
}

/// The status of an event (`STATUS`); `None` for a value RFC 5545 does not give events.
fn status(value: &str) -> Option<EventStatus> {
    match value.trim().to_ascii_uppercase().as_str() {
        "CONFIRMED" => Some(EventStatus::Confirmed),
        "TENTATIVE" => Some(EventStatus::Tentative),
        "CANCELLED" => Some(EventStatus::Cancelled),
        _ => None,
    }
}

/// Where the event is: `GEO:lat;lon` (RFC 5545 section 3.8.1.6), else the `geo:` URI of Apple's
/// `X-APPLE-STRUCTURED-LOCATION` (RFC 5870: `geo:lat,lon[,alt][;u=…]`). A value that is not two
/// numbers in range is none.
fn geo(props: &[Property<'_>]) -> Option<Geo> {
    let point = |lat: &str, lon: &str| Geo::new(lat.trim().parse().ok()?, lon.trim().parse().ok()?);
    let stored = find(props, "GEO").and_then(|p| {
        // Some programs write a comma instead of the semicolon.
        let (lat, lon) = p.value.split_once(';').or_else(|| p.value.split_once(','))?;
        point(lat, lon)
    });
    stored.or_else(|| {
        let apple = find(props, "X-APPLE-STRUCTURED-LOCATION")?.value.trim();
        let coords = strip_prefix_ignore_case(apple, "geo:")?.split(';').next()?;
        let mut parts = coords.split(',');
        point(parts.next()?, parts.next()?)
    })
}

/// `text` without `prefix`, whatever the case of the prefix.
fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &text[prefix.len()..])
}

/// The person an `ORGANIZER` or `ATTENDEE` names: `CN` as the name, the `mailto:` address (else
/// the `EMAIL` parameter of RFC 7986), `PARTSTAT` and `ROLE`; unknown values are the defaults.
fn person(prop: &Property<'_>) -> Attendee {
    let address = strip_prefix_ignore_case(prop.value.trim(), "mailto:").or_else(|| prop.param("EMAIL"));
    let partstat = match prop.param("PARTSTAT").map(|v| v.trim().to_ascii_uppercase()).as_deref() {
        Some("ACCEPTED") => PartStat::Accepted,
        Some("DECLINED") => PartStat::Declined,
        Some("TENTATIVE") => PartStat::Tentative,
        Some("DELEGATED") => PartStat::Delegated,
        _ => PartStat::NeedsAction,
    };
    let role = match prop.param("ROLE").map(|v| v.trim().to_ascii_uppercase()).as_deref() {
        Some("CHAIR") => AttendeeRole::Chair,
        Some("OPT-PARTICIPANT") => AttendeeRole::Optional,
        Some("NON-PARTICIPANT") => AttendeeRole::NonParticipant,
        _ => AttendeeRole::Required,
    };
    Attendee {
        name: prop.param("CN").map(param_text).filter(|name| !name.is_empty()),
        email: address.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()),
        partstat,
        role,
    }
}

/// A parameter value as text: trimmed, with the `^` escapes of RFC 6868 (`^n` line break, `^'`
/// double quote, `^^` caret) resolved.
fn param_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.trim().chars();
    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\'') => out.push('"'),
            Some('^') => out.push('^'),
            Some(other) => out.extend(['^', other]),
            None => out.push('^'),
        }
    }
    out
}

/// The reminder one `VALARM`'s properties describe: `ACTION:DISPLAY` or `AUDIO` with a readable
/// `TRIGGER`. `None` for any other alarm, which the model does not hold (and a save keeps).
pub(crate) fn alarm(props: &[Property<'_>]) -> Option<Alarm> {
    let action = find(props, "ACTION")?.value.trim();
    if !["DISPLAY", "AUDIO"].iter().any(|known| action.eq_ignore_ascii_case(known)) {
        return None;
    }
    let trigger = find(props, "TRIGGER")?;
    let absolute = trigger.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME"));
    let trigger = if absolute {
        let naive = NaiveDateTime::parse_from_str(trigger.value.trim().strip_suffix('Z')?, "%Y%m%dT%H%M%S").ok()?;
        AlarmTrigger::At(naive.and_utc())
    } else {
        let offset = parse_duration(trigger.value)?;
        let of = match trigger.param("RELATED") {
            Some(related) if related.eq_ignore_ascii_case("END") => AlarmAnchor::End,
            _ => AlarmAnchor::Start,
        };
        // A negative offset fires before; whole minutes only, toward zero.
        AlarmTrigger::Before { minutes: -offset.num_minutes(), of }
    };
    let style = find(props, "X-TORRO-STYLE").and_then(|p| match p.value.trim() {
        v if v.eq_ignore_ascii_case("FULLSCREEN") => Some(AlarmStyle::FullScreen),
        v if v.eq_ignore_ascii_case("NOTIFICATION") => Some(AlarmStyle::Notification),
        _ => None,
    });
    Some(Alarm { trigger, style })
}

/// Free or busy from `TRANSP`: only `TRANSPARENT` is free; a missing property or any other value
/// (RFC 5545 knows only these two) counts as `OPAQUE`, the default.
fn transparency(prop: Option<&Property<'_>>) -> Transparency {
    match prop {
        Some(prop) if prop.value.trim().eq_ignore_ascii_case("TRANSPARENT") => Transparency::Transparent,
        _ => Transparency::Opaque,
    }
}

fn add(time: EventTime, duration: Duration) -> EventTime {
    match time {
        EventTime::Date(date) => EventTime::Date(date + duration),
        EventTime::DateTime(at) => EventTime::DateTime(at + duration),
    }
}

/// A `DATE`, a UTC time (`…Z`), a time in the IANA zone its `TZID` names ([`zone`]) or, for a
/// floating time or a zone chrono-tz does not know, a time in the computer's local zone.
fn parse_time(prop: &Property<'_>) -> Option<EventTime> {
    let value = prop.value.trim();
    if prop.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE")) || value.len() == 8 {
        return NaiveDate::parse_from_str(value, "%Y%m%d").ok().map(EventTime::Date);
    }
    if let Some(utc) = value.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(EventTime::DateTime(naive.and_utc()));
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    if let Some(tz) = prop.param("TZID").and_then(zone) {
        return Some(EventTime::DateTime(in_zone(tz, naive)));
    }
    // Floating time, or a zone only the object's VTIMEZONE explains: the computer's local time.
    let local = Local.from_local_datetime(&naive).earliest()?;
    Some(EventTime::DateTime(local.with_timezone(&Utc)))
}

/// The instant the wall-clock time `naive` names in `tz`, as RFC 5545 section 3.3.5 reads it: a
/// time that occurs twice (the hour before winter time) is the first one, a time the clocks skip
/// (the hour before summer time) counts with the offset before the gap (02:30 becomes 03:30).
pub(crate) fn in_zone(tz: Tz, naive: NaiveDateTime) -> DateTime<Utc> {
    match tz.from_local_datetime(&naive).earliest() {
        Some(at) => at.with_timezone(&Utc),
        None => {
            // In the gap: the offset a day before is the one before it (zones change at most
            // twice a year).
            let before = tz.offset_from_utc_datetime(&(naive - Duration::days(1))).fix();
            (naive - Duration::seconds(i64::from(before.local_minus_utc()))).and_utc()
        }
    }
}

/// `P1D`, `PT1H30M`, `-PT15M`, `P2W` …
fn parse_duration(value: &str) -> Option<Duration> {
    let (sign, rest) = match value.trim().strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, value.trim().trim_start_matches('+')),
    };
    let rest = rest.strip_prefix('P')?;
    let mut seconds = 0i64;
    let mut number = String::new();
    for c in rest.chars() {
        match c {
            '0'..='9' => number.push(c),
            'T' => {}
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                seconds += n * match unit {
                    'W' => 7 * 86_400,
                    'D' => 86_400,
                    'H' => 3_600,
                    'M' => 60,
                    'S' => 1,
                    _ => return None,
                };
            }
        }
    }
    Some(Duration::seconds(sign * seconds))
}

/// Joins folded lines (a line break followed by a space or tab continues the line). Bare LF
/// line ends are accepted like CRLF; empty lines are dropped.
pub(crate) fn unfold(ics: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in ics.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        match (raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')), lines.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ if raw.is_empty() => {}
            _ => lines.push(raw.to_string()),
        }
    }
    lines
}

/// One unfolded content line as a [`Property`]; `None` when it has no value part.
pub(crate) fn parse_line(line: &str) -> Option<Property<'_>> {
    // The value starts at the first colon outside a quoted parameter value.
    let mut quoted = false;
    let colon = line.char_indices().find_map(|(i, c)| match c {
        '"' => {
            quoted = !quoted;
            None
        }
        ':' if !quoted => Some(i),
        _ => None,
    })?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?;
    let params = parts.filter_map(|p| p.split_once('=')).map(|(k, v)| (k, v.trim_matches('"'))).collect();
    Some(Property { name, params, value })
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `20261011T000000Z`, the form CalDAV time ranges use.
pub fn format_utc(at: DateTime<Utc>) -> String {
    at.format("%Y%m%dT%H%M%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:abc\r\nDTSTART:20261011T080000Z\r\nDTEND:20261011T093000Z\r\nSUMMARY:Workshop\\, Teil 1\r\nLOCATION:Raum\r\n  3\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nDESCRIPTION:Alarm\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:day\r\nDTSTART;VALUE=DATE:20261012\r\nDURATION:P1D\r\nSUMMARY:Urlaub\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:gone\r\nSTATUS:CANCELLED\r\nDTSTART:20261011T080000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn reads_timed_all_day_and_cancelled() {
        let events = parse_events(SAMPLE, "/cal/");
        assert_eq!(events.len(), 3, "cancelled events are read since TE.2");
        assert_eq!(events[2].status, Some(EventStatus::Cancelled));
        assert_eq!((events[0].status, events[0].time_zone.as_deref()), (None, None), "UTC times name no zone");
        let first = &events[0];
        assert_eq!(first.summary, "Workshop, Teil 1");
        assert_eq!(first.location.as_deref(), Some("Raum 3"));
        assert_eq!(first.description, None, "the alarm's description is not the event's");
        assert!(matches!(first.start, EventTime::DateTime(at) if format_utc(at) == "20261011T080000Z"));
        let day = &events[1];
        assert!(day.all_day());
        assert_eq!(day.end, NaiveDate::from_ymd_opt(2026, 10, 13).map(EventTime::Date));
    }

    #[test]
    fn transp_tells_free_from_busy() {
        let wrap = |extra: &str| {
            format!(
                "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\nDTSTART:20261011T080000Z\r\n{extra}END:VEVENT\r\nEND:VCALENDAR\r\n"
            )
        };
        let read = |extra: &str| parse_events(&wrap(extra), "/cal/").remove(0).transparency;
        assert_eq!(read(""), Transparency::Opaque, "missing means busy");
        assert_eq!(read("TRANSP:OPAQUE\r\n"), Transparency::Opaque);
        assert_eq!(read("TRANSP:TRANSPARENT\r\n"), Transparency::Transparent);
        assert_eq!(read("transp: transparent\r\n"), Transparency::Transparent, "any case, spaces trimmed");
        assert_eq!(read("TRANSP:X-MAYBE\r\n"), Transparency::Opaque, "unknown values are busy");
        let alarm = "BEGIN:VALARM\r\nTRANSP:TRANSPARENT\r\nEND:VALARM\r\n";
        assert_eq!(read(alarm), Transparency::Opaque, "an alarm's TRANSP is not the event's");
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(parse_duration("-PT15M"), Some(Duration::minutes(-15)));
        assert_eq!(parse_duration("P1W"), Some(Duration::days(7)));
    }

    const TODOS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
BEGIN:VTODO\r\nUID:done-1\r\nSUMMARY:Steuer machen\r\nDUE;VALUE=DATE:20261015\r\nPRIORITY:1\r\n\
STATUS:COMPLETED\r\nCOMPLETED:20261010T120000Z\r\nDESCRIPTION:Belege\\, Quittungen\\nund Auszüge\r\nEND:VTODO\r\n\
BEGIN:VTODO\r\nUID:child-1\r\nSUMMARY:Belege sortieren\r\nDUE:20261012T090000Z\r\nPRIORITY:0\r\n\
RELATED-TO;RELTYPE=PARENT:done-1\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\n\
DESCRIPTION:Erinnerung\r\nEND:VALARM\r\nEND:VTODO\r\n\
BEGIN:VTODO\r\nUID:plain-1\r\nSUMMARY:Milch\r\nRELATED-TO:done-1\r\nSTATUS:NEEDS-ACTION\r\nEND:VTODO\r\n\
BEGIN:VTODO\r\nUID:gone\r\nSTATUS:CANCELLED\r\nEND:VTODO\r\n\
BEGIN:VEVENT\r\nUID:not-a-task\r\nDTSTART:20261011T080000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn reads_todos() {
        let todos = parse_todos(TODOS, "/tasks/");
        assert_eq!(todos.len(), 3, "cancelled tasks and events are skipped");

        let done = &todos[0];
        assert_eq!(done.uid, "done-1");
        assert_eq!(done.collection, "/tasks/");
        assert_eq!(done.summary, "Steuer machen");
        assert_eq!(done.due, NaiveDate::from_ymd_opt(2026, 10, 15).map(EventTime::Date));
        assert_eq!(done.priority, Some(1));
        assert!(done.completed);
        assert_eq!(done.parent, None);
        assert_eq!(done.description.as_deref(), Some("Belege, Quittungen\nund Auszüge"));

        let child = &todos[1];
        assert_eq!(child.uid, "child-1");
        assert!(matches!(child.due, Some(EventTime::DateTime(at)) if format_utc(at) == "20261012T090000Z"));
        assert_eq!(child.priority, None, "priority 0 is undefined");
        assert!(!child.completed);
        assert_eq!(child.parent.as_deref(), Some("done-1"));
        assert_eq!(child.description, None, "the alarm's description is not the task's");

        let plain = &todos[2];
        assert_eq!(plain.due, None);
        assert_eq!(plain.parent.as_deref(), Some("done-1"), "RELATED-TO without RELTYPE is a parent");
        assert!(!plain.completed);
    }

    #[test]
    fn reads_url_categories_sort_order_and_link() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:x\r\nURL:https://github.com/mahype/TorroWorkplace/issues/123 \r\n\
CATEGORIES:Work,Finanzen\\, Steuer, ,Privat\r\ncategories:Later\r\nX-APPLE-SORT-ORDER:-42\r\n\
X-TORRO-LINK:github:mahype/TorroWorkplace#123\r\nBEGIN:VALARM\r\nCATEGORIES:Alarm\r\nEND:VALARM\r\n\
END:VTODO\r\nEND:VCALENDAR\r\n";
        let todo = parse_todos(ics, "/tasks/").remove(0);
        assert_eq!(todo.url.as_deref(), Some("https://github.com/mahype/TorroWorkplace/issues/123"));
        assert_eq!(todo.categories, ["Work", "Finanzen, Steuer", "Privat", "Later"], "the alarm's are not the task's");
        assert_eq!(todo.sort_order, Some(-42));
        assert_eq!(todo.link, Some(Link::github("mahype/TorroWorkplace", 123)));

        let plain = parse_todos(TODOS, "/tasks/").remove(2);
        assert_eq!((plain.url, plain.categories, plain.sort_order, plain.link), (None, Vec::new(), None, None));
        let odd = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:y\r\nURL:\r\nX-APPLE-SORT-ORDER:soon\r\nX-TORRO-LINK: \r\n\
END:VTODO\r\nEND:VCALENDAR\r\n";
        let odd = parse_todos(odd, "/tasks/").remove(0);
        assert_eq!((odd.url, odd.sort_order, odd.link), (None, None, None), "empty or unreadable values are none");
    }

    #[test]
    fn completed_property_marks_a_task_done() {
        let ics =
            "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:x\r\nCOMPLETED:20261010T120000Z\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let todos = parse_todos(ics, "/tasks/");
        assert_eq!(todos.len(), 1);
        assert!(todos[0].completed);
    }

    #[test]
    fn series_are_told_by_rule_dates_or_changed_occurrence() {
        let wrap = |component: &str, extra: &str| {
            format!(
                "BEGIN:VCALENDAR\r\nBEGIN:{component}\r\nUID:x\r\nDTSTART:20261011T080000Z\r\n{extra}END:{component}\r\nEND:VCALENDAR\r\n"
            )
        };
        assert!(is_series(&wrap("VEVENT", "RRULE:FREQ=WEEKLY\r\n")));
        assert!(is_series(&wrap("VEVENT", "RDATE:20261018T080000Z\r\n")));
        assert!(is_series(&wrap("VEVENT", "RECURRENCE-ID:20261018T080000Z\r\n")));
        assert!(is_series(&wrap("VTODO", "rrule:FREQ=DAILY\r\n")), "names are case-insensitive");
        assert!(!is_series(&wrap("VEVENT", "")));
        assert!(!is_series(&wrap("VTODO", "SUMMARY:RRULE in the text\r\n")));
        assert!(!is_series(SAMPLE), "single events, an alarm and a cancelled one");
        assert!(!is_series(TODOS));
        // A rule inside an alarm is not the event's.
        assert!(!is_series(&wrap("VEVENT", "BEGIN:VALARM\r\nRRULE:FREQ=DAILY\r\nEND:VALARM\r\n")));
    }

    #[test]
    fn events_with_extra_dates_are_recurring() {
        // A server that does not expand series hands over RDATE-only ones as they are.
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\nDTSTART:20261011T080000Z\r\n\
RDATE:20261018T080000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_events(ics, "/cal/");
        assert_eq!(events.len(), 1);
        assert!(events[0].recurring);
    }

    // Reminders (task ER1.1).

    const NEXTCLOUD_ALARMS: &str = include_str!("../tests/fixtures/nextcloud-event-alarms.ics");
    const APPLE_ALARMS: &str = include_str!("../tests/fixtures/apple-event-alarms.ics");
    const THUNDERBIRD_EVENT: &str = include_str!("../tests/fixtures/thunderbird-event.ics");

    fn before(minutes: i64, of: AlarmAnchor) -> Alarm {
        Alarm { trigger: AlarmTrigger::Before { minutes, of }, style: None }
    }

    fn alarms_of(ics: &str) -> Vec<Alarm> {
        parse_events(ics, "/cal/").into_iter().next().expect("an event").alarms
    }

    #[test]
    fn reads_the_reminders_nextcloud_writes() {
        let at = NaiveDate::from_ymd_opt(2026, 10, 15).and_then(|d| d.and_hms_opt(7, 0, 0)).expect("time").and_utc();
        assert_eq!(
            alarms_of(NEXTCLOUD_ALARMS),
            [
                before(15, AlarmAnchor::Start),
                before(0, AlarmAnchor::End),
                Alarm { trigger: AlarmTrigger::At(at), style: None },
            ],
            "the mail alarm and the broken trigger are not the model's"
        );
    }

    #[test]
    fn reads_the_reminders_apple_and_thunderbird_write() {
        assert_eq!(
            alarms_of(APPLE_ALARMS),
            [before(30, AlarmAnchor::Start), before(24 * 60, AlarmAnchor::Start)],
            "a sound counts, Apple's ACTION:NONE placeholder does not"
        );
        assert_eq!(alarms_of(THUNDERBIRD_EVENT), [before(15, AlarmAnchor::Start)], "TRIGGER;VALUE=DURATION");
    }

    #[test]
    fn reads_the_style_and_odd_triggers() {
        let wrap = |alarms: &str| {
            format!(
                "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\nDTSTART:20261011T080000Z\r\n{alarms}END:VEVENT\r\nEND:VCALENDAR\r\n"
            )
        };
        let alarm = |body: &str| format!("BEGIN:VALARM\r\n{body}END:VALARM\r\n");
        let ics = wrap(
            &[
                alarm("ACTION:DISPLAY\r\nTRIGGER:-PT10M\r\nX-TORRO-STYLE:FULLSCREEN\r\n"),
                alarm("action:display\r\nTRIGGER:-PT90S\r\nX-TORRO-STYLE:notification\r\n"),
                alarm("ACTION:DISPLAY\r\nTRIGGER:PT5M\r\nX-TORRO-STYLE:SOMETHING\r\n"),
                alarm("TRIGGER:-PT1H\r\n"),
                alarm("ACTION:DISPLAY\r\nDESCRIPTION:no trigger\r\n"),
            ]
            .concat(),
        );
        assert_eq!(
            alarms_of(&ics),
            [
                Alarm { style: Some(AlarmStyle::FullScreen), ..before(10, AlarmAnchor::Start) },
                Alarm { style: Some(AlarmStyle::Notification), ..before(1, AlarmAnchor::Start) },
                before(-5, AlarmAnchor::Start),
            ],
            "seconds are dropped, after the start is negative, unknown styles and alarms without ACTION or TRIGGER are not read"
        );
        let event = parse_events(&ics, "/cal/").remove(0);
        assert_eq!(event.description, None, "an alarm's description is not the event's");
    }

    // New fields, time zones and attendees (task TE.2).

    const NEXTCLOUD_BERLIN: &str = include_str!("../tests/fixtures/nextcloud-event-berlin.ics");
    const NEXTCLOUD_CANCELLED: &str = include_str!("../tests/fixtures/nextcloud-event-cancelled.ics");
    const NEXTCLOUD_ATTENDEES: &str = include_str!("../tests/fixtures/nextcloud-event-attendees.ics");
    const APPLE_PLACE: &str = include_str!("../tests/fixtures/apple-structured-location.ics");

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> EventTime {
        EventTime::DateTime(Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).single().expect("a time"))
    }

    fn one(ics: &str) -> Event {
        parse_events(ics, "/cal/").into_iter().next().expect("an event")
    }

    #[test]
    fn tzid_times_are_converted_with_the_zone_and_the_zone_is_kept() {
        // Summer time: 10:00 in Berlin is 08:00 UTC.
        let summer = one(NEXTCLOUD_BERLIN);
        assert_eq!(summer.start, utc(2026, 10, 13, 8, 0));
        assert_eq!(summer.end, Some(utc(2026, 10, 13, 9, 30)));
        assert_eq!(summer.time_zone.as_deref(), Some("Europe/Berlin"));
        // Winter time: 09:00 in Berlin is 08:00 UTC.
        let winter = one(NEXTCLOUD_CANCELLED);
        assert_eq!(winter.start, utc(2026, 1, 20, 8, 0));
        assert_eq!(winter.time_zone.as_deref(), Some("Europe/Berlin"));
    }

    #[test]
    fn zone_names_behind_a_prefix_are_found_and_windows_names_are_not() {
        assert_eq!(zone("Europe/Berlin"), Some(Tz::Europe__Berlin));
        assert_eq!(zone(" America/Argentina/Buenos_Aires "), Some(Tz::America__Argentina__Buenos_Aires));
        assert_eq!(zone("/mozilla.org/20050126_1/Europe/Berlin"), Some(Tz::Europe__Berlin));
        assert_eq!(zone("/citadel.org/20190914_1/America/New_York"), Some(Tz::America__New_York));
        assert_eq!(zone("W. Europe Standard Time"), None);
        assert_eq!(zone(""), None);
        // Unknown names are read as local time, as before, and name no zone.
        let windows = NEXTCLOUD_BERLIN.replace("TZID=Europe/Berlin", "TZID=W. Europe Standard Time");
        assert_eq!(one(&windows).time_zone, None);
        let prefixed = NEXTCLOUD_BERLIN.replace("TZID=Europe/Berlin", "TZID=/mozilla.org/20050126_1/Europe/Berlin");
        let event = one(&prefixed);
        assert_eq!((event.start, event.time_zone.as_deref()), (utc(2026, 10, 13, 8, 0), Some("Europe/Berlin")));
    }

    #[test]
    fn times_around_the_change_of_the_clock_follow_rfc_5545() {
        let berlin = Tz::Europe__Berlin;
        let naive = |d: u32, mo: u32, h: u32, mi: u32| {
            NaiveDate::from_ymd_opt(2026, mo, d).and_then(|d| d.and_hms_opt(h, mi, 0)).expect("a time")
        };
        let at = |y, mo, d, h, mi| Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).single().expect("a time");
        // 02:30 on 29 March does not exist: counted with the offset before the gap (+01:00).
        assert_eq!(in_zone(berlin, naive(29, 3, 2, 30)), at(2026, 3, 29, 1, 30));
        // 02:30 on 25 October happens twice: the first one (+02:00).
        assert_eq!(in_zone(berlin, naive(25, 10, 2, 30)), at(2026, 10, 25, 0, 30));
        assert_eq!(in_zone(berlin, naive(25, 10, 3, 30)), at(2026, 10, 25, 2, 30));
    }

    #[test]
    fn reads_url_status_and_a_cancelled_event() {
        let event = one(NEXTCLOUD_BERLIN);
        assert_eq!(event.url.as_deref(), Some("https://wiki.example.org/planung/2026-10-13"));
        assert_eq!(event.status, Some(EventStatus::Confirmed));
        assert_eq!((event.geo, event.organizer.as_deref(), event.attendees.len()), (None, None, 0));
        let cancelled = one(NEXTCLOUD_CANCELLED);
        assert_eq!(cancelled.status, Some(EventStatus::Cancelled));
        assert_eq!(cancelled.summary, "Jahresgespräch");
        assert!(!cancelled.busy(), "a cancelled event keeps no time busy");
        let status = |value: &str| one(&NEXTCLOUD_BERLIN.replace("STATUS:CONFIRMED", value)).status;
        assert_eq!(status("status: tentative "), Some(EventStatus::Tentative));
        assert_eq!(status("STATUS:X-POSTPONED"), None, "an unknown status is none");
        assert_eq!(status("STATUS:NEEDS-ACTION"), None, "a task's status is not an event's");
    }

    #[test]
    fn reads_the_place_from_geo_or_from_apple() {
        let geo = one(NEXTCLOUD_ATTENDEES).geo.expect("GEO");
        assert_eq!((geo.lat, geo.lon), (52.520008, 13.404954));
        let apple = one(APPLE_PLACE);
        let place = apple.geo.expect("Apple's place");
        assert_eq!((place.lat, place.lon), (52.516275, 13.377704));
        assert_eq!(apple.location.as_deref(), Some("Brandenburger Tor\nPariser Platz, 10117 Berlin, Deutschland"));
        // GEO wins over Apple's place; broken values are none.
        let both = APPLE_PLACE.replace("SEQUENCE:0", "GEO:48.137154;11.576124");
        assert_eq!(one(&both).geo.map(|g| (g.lat, g.lon)), Some((48.137154, 11.576124)));
        for broken in ["GEO:91;13", "GEO:52.5", "GEO:nan;13", "GEO:north;east"] {
            let ics = NEXTCLOUD_ATTENDEES.replace("GEO:52.520008;13.404954", broken);
            assert_eq!(one(&ics).geo, None, "{broken}");
        }
        let comma = NEXTCLOUD_ATTENDEES.replace("GEO:52.520008;13.404954", "GEO:52.5,13.4");
        assert_eq!(one(&comma).geo.map(|g| (g.lat, g.lon)), Some((52.5, 13.4)), "a comma is forgiven");
        let with_params = APPLE_PLACE.replace("geo:52.516275,13.377704", "GEO:52.5,13.4,34;u=35");
        assert_eq!(one(&with_params).geo.map(|g| (g.lat, g.lon)), Some((52.5, 13.4)), "altitude and u= are left out");
    }

    #[test]
    fn reads_organizer_and_attendees() {
        let event = one(NEXTCLOUD_ATTENDEES);
        let anna = Attendee {
            name: Some("Anna Beispiel".into()),
            email: Some("anna@example.org".into()),
            partstat: PartStat::NeedsAction,
            role: AttendeeRole::Required,
        };
        assert_eq!(event.organizer.as_deref(), Some(&anna));
        assert_eq!(
            event.attendees,
            [
                Attendee { partstat: PartStat::Accepted, role: AttendeeRole::Chair, ..anna.clone() },
                Attendee {
                    name: Some("Beispiel, Ben".into()),
                    email: Some("ben@example.org".into()),
                    partstat: PartStat::Tentative,
                    role: AttendeeRole::Required,
                },
                Attendee {
                    name: None,
                    email: Some("carla@example.org".into()),
                    partstat: PartStat::NeedsAction,
                    role: AttendeeRole::Optional,
                },
                Attendee {
                    name: Some("Raum 2".into()),
                    email: None,
                    partstat: PartStat::Accepted,
                    role: AttendeeRole::NonParticipant,
                },
            ]
        );
        let declined = one(NEXTCLOUD_CANCELLED).attendees;
        assert_eq!(declined[1].partstat, PartStat::Declined);
        // RFC 6868 escapes and the EMAIL parameter of RFC 7986.
        let odd = NEXTCLOUD_BERLIN.replace(
            "STATUS:CONFIRMED",
            "ATTENDEE;CN=Dr. ^'Q^' Muster;EMAIL=q@example.org;PARTSTAT=X-UNKNOWN:urn:uuid:1",
        );
        let q = one(&odd).attendees.remove(0);
        assert_eq!((q.name.as_deref(), q.email.as_deref()), (Some("Dr. \"Q\" Muster"), Some("q@example.org")));
        assert_eq!(q.partstat, PartStat::NeedsAction, "unknown answers are not answers");
    }

    #[test]
    fn reads_the_recurrence_id() {
        let ics = NEXTCLOUD_BERLIN.replace("STATUS:CONFIRMED", "RECURRENCE-ID;TZID=Europe/Berlin:20261013T100000");
        let event = one(&ics);
        assert_eq!(event.recurrence_id, Some(utc(2026, 10, 13, 8, 0)));
        assert!(event.recurring);
        assert_eq!(one(NEXTCLOUD_BERLIN).recurrence_id, None);
    }

    // The stored object of a series (task TE.3).

    fn facts(extra: &str) -> ObjectFacts {
        ObjectFacts::read(&NEXTCLOUD_BERLIN.replace("STATUS:CONFIRMED", extra))
    }

    #[test]
    fn extra_dates_and_exclusion_rules_make_the_rule_custom() {
        let weekly = facts("RRULE:FREQ=WEEKLY;BYDAY=TU").repeat.expect("a rule");
        assert!(!weekly.custom);
        assert_eq!(facts("RRULE:FREQ=WEEKLY;BYDAY=TU").zone.as_deref(), Some("Europe/Berlin"));
        for extra in [
            "RRULE:FREQ=WEEKLY;BYDAY=TU\r\nRDATE;TZID=Europe/Berlin:20261016T100000",
            "RRULE:FREQ=WEEKLY;BYDAY=TU\r\nEXRULE:FREQ=MONTHLY;BYDAY=1TU",
            "RRULE:FREQ=WEEKLY;BYDAY=TU\r\nRRULE:FREQ=WEEKLY;BYDAY=TH",
        ] {
            let repeat = facts(extra).repeat.expect("a rule");
            assert!(repeat.custom, "{extra}");
            assert_eq!(repeat.rule, "FREQ=WEEKLY;BYDAY=TU", "the first rule's text: {extra}");
        }
        let dates_only = facts("RDATE;TZID=Europe/Berlin:20261016T100000").repeat.expect("a series");
        assert!(dates_only.custom && dates_only.rule.is_empty());
        assert_eq!(facts("SEQUENCE:3"), ObjectFacts { zone: Some("Europe/Berlin".into()), ..ObjectFacts::default() });
    }

    #[test]
    fn an_object_of_changed_occurrences_only_has_no_rule() {
        let only = facts("RECURRENCE-ID;TZID=Europe/Berlin:20261013T090000");
        assert_eq!((&only.repeat, &only.zone), (&None, &None));
        let key = utc(2026, 10, 13, 7, 0);
        assert_eq!(
            only.exceptions,
            [Exception { recurrence_id: key, start: utc(2026, 10, 13, 8, 0), zone: Some("Europe/Berlin".into()) }]
        );
        let mut occurrence = Event { time_zone: None, recurring: false, ..one(NEXTCLOUD_BERLIN) };
        only.complete(&mut occurrence);
        assert_eq!(occurrence.recurrence_id, Some(key), "found by its start");
        assert_eq!((occurrence.time_zone.as_deref(), occurrence.recurring), (Some("Europe/Berlin"), true));
    }
}
