//! Just enough iCalendar (RFC 5545) to read events (VEVENT) and tasks (VTODO).
//!
//! The server expands recurring events for us (`<C:expand>` in the calendar query), so
//! every occurrence arrives as its own VEVENT with times in UTC. Times with a TZID that
//! slip through anyway are read as local time; a full time zone database comes with the
//! sync engine.
//!
//! An event's reminders (task ER1.1, `docs/erinnerungen.md`) are its `VALARM` components with
//! `ACTION:DISPLAY` or `AUDIO` ([`alarm`]). Mail alarms, `ACTION:NONE` (Apple's placeholder) and
//! alarms whose `TRIGGER` cannot be read are not the model's: they are left out, not reported.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use torro_pim::model::Link;
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

/// The events in one iCalendar object (one `calendar-data`), skipping cancelled ones.
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

/// The event one VEVENT's own properties and its `VALARM`s describe; `None` for a cancelled one
/// or one without a readable `DTSTART`.
pub(crate) fn to_event(props: &[Property<'_>], alarms: &[Vec<Property<'_>>], collection: &str) -> Option<Event> {
    let get = |name: &str| find(props, name);
    if get("STATUS").is_some_and(|p| p.value.eq_ignore_ascii_case("CANCELLED")) {
        return None;
    }
    let start = parse_time(get("DTSTART")?)?;
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
        // Read from the server from task TE.2 (fields) and TE.3 (series) on.
        recurrence_id: None,
        repeat: None,
        time_zone: None,
        url: None,
        status: None,
        geo: None,
        organizer: None,
        attendees: Vec::new(),
    })
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

fn parse_time(prop: &Property<'_>) -> Option<EventTime> {
    let value = prop.value.trim();
    if prop.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE")) || value.len() == 8 {
        return NaiveDate::parse_from_str(value, "%Y%m%d").ok().map(EventTime::Date);
    }
    if let Some(utc) = value.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(EventTime::DateTime(naive.and_utc()));
    }
    // Floating or TZID time: read as local time for now.
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    let local = Local.from_local_datetime(&naive).earliest()?;
    Some(EventTime::DateTime(local.with_timezone(&Utc)))
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
    fn reads_timed_all_day_and_skips_cancelled() {
        let events = parse_events(SAMPLE, "/cal/");
        assert_eq!(events.len(), 2);
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
}
