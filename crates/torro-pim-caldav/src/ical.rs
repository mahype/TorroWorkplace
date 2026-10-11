//! Just enough iCalendar (RFC 5545) to read events.
//!
//! The server expands recurring events for us (`<C:expand>` in the calendar query), so
//! every occurrence arrives as its own VEVENT with times in UTC. Times with a TZID that
//! slip through anyway are read as local time; a full time zone database comes with the
//! sync engine.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use torro_pim::{Event, EventTime};

/// One content line: `NAME;PARAM=VALUE:value`.
struct Property<'a> {
    name: &'a str,
    params: Vec<(&'a str, &'a str)>,
    value: &'a str,
}

impl Property<'_> {
    fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| *value)
    }
}

/// The events in one iCalendar object (one `calendar-data`), skipping cancelled ones.
pub fn parse_events(ics: &str, collection: &str) -> Vec<Event> {
    let lines = unfold(ics);
    let mut events = Vec::new();
    let mut current: Option<Vec<Property<'_>>> = None;
    let mut depth = 0usize;
    for line in &lines {
        let Some(prop) = parse_line(line) else { continue };
        match (prop.name, current.as_mut()) {
            ("BEGIN", None) if prop.value.eq_ignore_ascii_case("VEVENT") => current = Some(Vec::new()),
            ("BEGIN", Some(_)) => depth += 1, // VALARM inside the event
            ("END", Some(_)) if depth > 0 => depth -= 1,
            ("END", Some(_)) => {
                if let Some(event) = current.take().and_then(|props| to_event(&props, collection)) {
                    events.push(event);
                }
            }
            (_, Some(props)) if depth == 0 => props.push(prop),
            _ => {}
        }
    }
    events
}

fn to_event(props: &[Property<'_>], collection: &str) -> Option<Event> {
    let get = |name: &str| props.iter().find(|p| p.name.eq_ignore_ascii_case(name));
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
        recurring: get("RECURRENCE-ID").is_some() || get("RRULE").is_some(),
        summary: get("SUMMARY").map(|p| unescape(p.value)).unwrap_or_default(),
        start,
        end,
        location: get("LOCATION").map(|p| unescape(p.value)).filter(|s| !s.is_empty()),
        description: get("DESCRIPTION").map(|p| unescape(p.value)).filter(|s| !s.is_empty()),
    })
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

/// Joins folded lines (a line break followed by a space or tab continues the line).
fn unfold(ics: &str) -> Vec<String> {
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

fn parse_line(line: &str) -> Option<Property<'_>> {
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
    fn durations() {
        assert_eq!(parse_duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(parse_duration("-PT15M"), Some(Duration::minutes(-15)));
        assert_eq!(parse_duration("P1W"), Some(Duration::days(7)));
    }
}
