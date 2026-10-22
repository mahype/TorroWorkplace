//! The rule of a series: iCalendar `RRULE` (RFC 5545 section 3.3.10) to [`Repeat`] and back (task
//! TE.3, `docs/termin-editor.md` decision 2).
//!
//! The server expands the occurrences (decision 1), so nothing here counts dates: [`read`] turns
//! the rule text into the fields the event editor shows, [`write`] turns the fields back into rule
//! text. A rule with a part the fields cannot hold (`BYSETPOS`, `BYHOUR`, `BYMINUTE`, `BYSECOND`,
//! `BYWEEKNO`, `BYYEARDAY`, an `X-` part, a frequency below a day, a value out of range …) is
//! read as far as it goes and marked [`Repeat::custom`]; its text stays in [`Repeat::rule`] and
//! [`write`] gives it back unchanged. A rule text from a client is written only when it passes
//! [`check`] (one line of the `RRULE` grammar), so it can never add lines to the object. Extra dates (`RDATE`) and exclusion rules (`EXRULE`) are not
//! part of the rule text: the reader of the stored object marks those (`crate::ical::ObjectFacts`).
//! The sentence the editor shows ("Every week on Tuesday") is the app's (task TE.7).

use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use torro_pim::EventTime;
use torro_pim::model::{Repeat, RepeatDay, RepeatEnd, RepeatFreq, Weekday};

use crate::ical;

/// The fields of the rule `rule` (the value of `RRULE`, without `RRULE:`), with the text itself in
/// [`Repeat::rule`]. `zone` is the zone of the series' `DTSTART`: an `UNTIL` without `Z` (which RFC
/// 5545 allows only with a floating or all-day start, but programs write anyway) counts in it, or
/// in the computer's zone when there is none.
///
/// Never fails: whatever cannot be held in the fields sets [`Repeat::custom`] and keeps its
/// default (`FREQ` missing or below a day reads as daily, a broken `INTERVAL` as 1, `COUNT`
/// together with `UNTIL` as the count). A `WKST` other than Monday only marks the rule custom where
/// it changes the dates (weekly with an interval above 1); elsewhere it is left out.
pub fn read(rule: &str, zone: Option<Tz>) -> Repeat {
    let rule = rule.trim();
    let mut repeat = Repeat {
        freq: RepeatFreq::Daily,
        interval: 1,
        weekdays: Vec::new(),
        month_days: Vec::new(),
        months: Vec::new(),
        end: RepeatEnd::Never,
        rule: rule.to_string(),
        custom: false,
    };
    let (mut has_freq, mut custom) = (false, false);
    let (mut count, mut until, mut week_start) = (None, None, None);
    for part in rule.split(';').map(str::trim).filter(|part| !part.is_empty()) {
        let Some((key, value)) = part.split_once('=') else {
            custom = true;
            continue;
        };
        let value = value.trim();
        let known = match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => freq(value).map(|freq| {
                repeat.freq = freq;
                has_freq = true;
            }),
            "INTERVAL" => value.parse::<u16>().ok().filter(|n| *n >= 1).map(|n| repeat.interval = n),
            "BYDAY" => list(value, weekday).map(|days| repeat.weekdays = days),
            "BYMONTHDAY" => list(value, month_day).map(|days| repeat.month_days = days),
            "BYMONTH" => list(value, month).map(|months| repeat.months = months),
            "COUNT" => value.parse::<u32>().ok().filter(|n| *n >= 1).map(|n| count = Some(n)),
            "UNTIL" => until_time(value, zone).map(|at| until = Some(at)),
            "WKST" => weekday_of(value).map(|day| week_start = Some(day)),
            // BYSETPOS, BYHOUR, BYMINUTE, BYSECOND, BYWEEKNO, BYYEARDAY, RSCALE, X-…
            _ => None,
        };
        custom |= known.is_none();
    }
    repeat.end = match (count, until) {
        (Some(n), None) => RepeatEnd::Count(n),
        (None, Some(at)) => RepeatEnd::Until(at),
        (None, None) => RepeatEnd::Never,
        // RFC 5545 forbids both; the count is the shorter promise to keep.
        (Some(n), Some(_)) => {
            custom = true;
            RepeatEnd::Count(n)
        }
    };
    let positioned = repeat.weekdays.iter().any(|day| day.nth.is_some());
    let weekly = repeat.freq == RepeatFreq::Weekly;
    custom |= !has_freq
        || (positioned && matches!(repeat.freq, RepeatFreq::Daily | RepeatFreq::Weekly))
        || (weekly && !repeat.month_days.is_empty())
        || (weekly && repeat.interval > 1 && week_start.is_some_and(|day| day != Weekday::Monday));
    repeat.custom = custom;
    repeat
}

/// The rule text (`RRULE` value, without `RRULE:`) for `repeat`:
///
/// - [`Repeat::custom`]: [`Repeat::rule`] as it is – the stored rule stays untouched (empty when the
///   series has extra dates only and no rule).
/// - [`Repeat::rule`] set and its fields unchanged (as [`read`] gives them): the stored text, so
///   an untouched rule keeps its order, its `WKST` and anything else the fields leave out.
/// - Otherwise a new rule from the fields: `FREQ`, `INTERVAL` (when above 1), `BYMONTH`,
///   `BYMONTHDAY`, `BYDAY`, then `COUNT` or `UNTIL`. An `UNTIL` day is written as a date, an
///   instant in UTC (`…Z`), as RFC 5545 asks for a start with a zone; the caller gives a day for
///   an all-day series and an instant for a timed one.
///
/// A [`Repeat::rule`] that fails [`check`] is never given back: a custom rule then writes nothing
/// (empty), any other is written from the fields. Which text counts as stored is the caller's
/// choice: [`crate::write`] passes the rule the server holds, never a client's text.
pub fn write(repeat: &Repeat) -> String {
    let stored = repeat.rule.trim();
    let writable = check(stored).is_ok();
    if repeat.custom {
        return if writable { stored.to_string() } else { String::new() };
    }
    if writable && !stored.is_empty() && same_fields(&read(stored, None), repeat) {
        return stored.to_string();
    }
    let mut parts = vec![format!("FREQ={}", freq_code(repeat.freq))];
    if repeat.interval > 1 {
        parts.push(format!("INTERVAL={}", repeat.interval));
    }
    if !repeat.months.is_empty() {
        parts.push(format!("BYMONTH={}", joined(&repeat.months, u8::to_string)));
    }
    if !repeat.month_days.is_empty() {
        parts.push(format!("BYMONTHDAY={}", joined(&repeat.month_days, i8::to_string)));
    }
    if !repeat.weekdays.is_empty() {
        parts.push(format!("BYDAY={}", joined(&repeat.weekdays, day_code)));
    }
    match repeat.end {
        RepeatEnd::Never => {}
        RepeatEnd::Count(n) => parts.push(format!("COUNT={n}")),
        RepeatEnd::Until(EventTime::Date(day)) => parts.push(format!("UNTIL={}", day.format("%Y%m%d"))),
        RepeatEnd::Until(EventTime::DateTime(at)) => parts.push(format!("UNTIL={}", ical::format_utc(at))),
    }
    parts.join(";")
}

/// The longest rule text a client may send ([`check`]); real rules are far shorter.
pub const MAX_RULE_CHARS: usize = 1000;

/// Whether `rule`, a rule text a client sent ([`Repeat::rule`]), is one line of the `RRULE` grammar
/// (RFC 5545 section 3.3.10) that may be written: empty (no text), or at most [`MAX_RULE_CHARS`]
/// characters of letters, digits, `;`, `=`, `,`, `+`, `-` and spaces, every part `NAME=VALUE` with
/// a name of letters, digits and `-`, and exactly one `FREQ` with a frequency RFC 5545 names.
///
/// This is the guard against a client smuggling lines into the stored object: a line break, a `:`
/// or any other character outside the grammar would let `ATTENDEE`, `VALARM` or `END:VEVENT` follow
/// the rule, and the server would send invitations for them. The core refuses such a text with
/// `error` before anything is written; the writer leaves it out as well ([`crate::write`]). The
/// error is a reason for people.
pub fn check(rule: &str) -> Result<(), &'static str> {
    if rule.chars().count() > MAX_RULE_CHARS {
        return Err("the rule text is too long");
    }
    // Checked before trimming: `trim` would hide a line break at either end.
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, ';' | '=' | ',' | '+' | '-' | ' ');
    if !rule.chars().all(allowed) {
        return Err("the rule text holds characters an RRULE cannot have: one line of letters, digits and ;=,+-");
    }
    let rule = rule.trim();
    if rule.is_empty() {
        return Ok(());
    }
    let mut freqs = 0;
    for part in rule.split(';').map(str::trim).filter(|part| !part.is_empty()) {
        let Some((name, value)) = part.split_once('=') else { return Err("every part of the rule is NAME=VALUE") };
        let (name, value) = (name.trim(), value.trim());
        let named = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !named || value.is_empty() || value.contains('=') {
            return Err("every part of the rule is NAME=VALUE");
        }
        if name.eq_ignore_ascii_case("FREQ") {
            let known = ["SECONDLY", "MINUTELY", "HOURLY", "DAILY", "WEEKLY", "MONTHLY", "YEARLY"];
            if !known.iter().any(|freq| value.eq_ignore_ascii_case(freq)) {
                return Err("the rule has no frequency RFC 5545 knows");
            }
            freqs += 1;
        }
    }
    if freqs != 1 {
        return Err("the rule needs exactly one FREQ");
    }
    Ok(())
}

/// The fields the editor shows are the same in `a` and `b` (text and `custom` aside).
fn same_fields(a: &Repeat, b: &Repeat) -> bool {
    (a.freq, a.interval.max(1), &a.weekdays, &a.month_days, &a.months, a.end)
        == (b.freq, b.interval.max(1), &b.weekdays, &b.month_days, &b.months, b.end)
}

fn joined<T>(items: &[T], show: impl Fn(&T) -> String) -> String {
    items.iter().map(show).collect::<Vec<_>>().join(",")
}

fn freq(value: &str) -> Option<RepeatFreq> {
    match value.to_ascii_uppercase().as_str() {
        "DAILY" => Some(RepeatFreq::Daily),
        "WEEKLY" => Some(RepeatFreq::Weekly),
        "MONTHLY" => Some(RepeatFreq::Monthly),
        "YEARLY" => Some(RepeatFreq::Yearly),
        // SECONDLY, MINUTELY, HOURLY: no field holds them.
        _ => None,
    }
}

fn freq_code(freq: RepeatFreq) -> &'static str {
    match freq {
        RepeatFreq::Daily => "DAILY",
        RepeatFreq::Weekly => "WEEKLY",
        RepeatFreq::Monthly => "MONTHLY",
        RepeatFreq::Yearly => "YEARLY",
    }
}

/// Every comma-separated item of `value` read with `item`; `None` when one cannot be read.
fn list<T>(value: &str, item: impl Fn(&str) -> Option<T>) -> Option<Vec<T>> {
    value.split(',').map(|part| item(part.trim())).collect()
}

/// `TU`, `4TU`, `+4TU`, `-1FR`: the n-th is 1 to 53 or -1 to -53.
fn weekday(text: &str) -> Option<RepeatDay> {
    let split = text.len().checked_sub(2)?;
    let (nth, code) = (text.get(..split)?, text.get(split..)?);
    let day = weekday_of(code)?;
    let nth = match nth {
        "" => None,
        nth => Some(nth.parse::<i8>().ok().filter(|n| *n != 0 && (-53..=53).contains(n))?),
    };
    Some(RepeatDay { day, nth })
}

fn weekday_of(code: &str) -> Option<Weekday> {
    match code.trim().to_ascii_uppercase().as_str() {
        "MO" => Some(Weekday::Monday),
        "TU" => Some(Weekday::Tuesday),
        "WE" => Some(Weekday::Wednesday),
        "TH" => Some(Weekday::Thursday),
        "FR" => Some(Weekday::Friday),
        "SA" => Some(Weekday::Saturday),
        "SU" => Some(Weekday::Sunday),
        _ => None,
    }
}

fn day_code(day: &RepeatDay) -> String {
    let code = match day.day {
        Weekday::Monday => "MO",
        Weekday::Tuesday => "TU",
        Weekday::Wednesday => "WE",
        Weekday::Thursday => "TH",
        Weekday::Friday => "FR",
        Weekday::Saturday => "SA",
        Weekday::Sunday => "SU",
    };
    match day.nth {
        Some(nth) => format!("{nth}{code}"),
        None => code.to_string(),
    }
}

/// 1 to 31 or -1 to -31.
fn month_day(text: &str) -> Option<i8> {
    text.parse::<i8>().ok().filter(|n| *n != 0 && (-31..=31).contains(n))
}

/// 1 to 12.
fn month(text: &str) -> Option<u8> {
    text.parse::<u8>().ok().filter(|n| (1..=12).contains(n))
}

/// `20261231` (a day), `20261231T225959Z` (UTC) or `20261231T235959` (in `zone`, else the
/// computer's zone).
fn until_time(value: &str, zone: Option<Tz>) -> Option<EventTime> {
    if value.len() == 8 {
        return NaiveDate::parse_from_str(value, "%Y%m%d").ok().map(EventTime::Date);
    }
    if let Some(utc) = value.strip_suffix('Z').or_else(|| value.strip_suffix('z')) {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(EventTime::DateTime(naive.and_utc()));
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    match zone {
        Some(tz) => Some(EventTime::DateTime(ical::in_zone(tz, naive))),
        None => Local.from_local_datetime(&naive).earliest().map(|at| EventTime::DateTime(at.with_timezone(&Utc))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(day: Weekday) -> RepeatDay {
        RepeatDay { day, nth: None }
    }

    /// The rule text comes out as it went in, both through the fields alone (a new rule) and with
    /// the stored text.
    fn round_trip(rule: &str) -> Repeat {
        let repeat = read(rule, None);
        assert!(!repeat.custom, "{rule} fits the fields");
        assert_eq!(write(&Repeat { rule: String::new(), ..repeat.clone() }), rule, "from the fields");
        assert_eq!(write(&repeat), rule, "the stored text");
        repeat
    }

    #[test]
    fn weekly_on_tuesday() {
        let repeat = round_trip("FREQ=WEEKLY;BYDAY=TU");
        assert_eq!((repeat.freq, repeat.interval, repeat.end), (RepeatFreq::Weekly, 1, RepeatEnd::Never));
        assert_eq!(repeat.weekdays, [day(Weekday::Tuesday)]);
        assert_eq!(repeat.rule, "FREQ=WEEKLY;BYDAY=TU");
    }

    #[test]
    fn every_workday_as_nextcloud_writes_it() {
        let repeat = round_trip("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR");
        let days: Vec<Weekday> = repeat.weekdays.iter().map(|d| d.day).collect();
        assert_eq!(days, [Weekday::Monday, Weekday::Tuesday, Weekday::Wednesday, Weekday::Thursday, Weekday::Friday]);
    }

    #[test]
    fn monthly_on_the_fourth_tuesday_and_on_the_last_day() {
        let fourth = round_trip("FREQ=MONTHLY;BYDAY=4TU");
        assert_eq!(fourth.weekdays, [RepeatDay { day: Weekday::Tuesday, nth: Some(4) }]);
        let last_friday = read("FREQ=MONTHLY;BYDAY=-1FR", None);
        assert_eq!(last_friday.weekdays, [RepeatDay { day: Weekday::Friday, nth: Some(-1) }]);
        assert_eq!(read("FREQ=MONTHLY;BYDAY=+4TU", None).weekdays, fourth.weekdays, "a plus sign is the same");
        let last = round_trip("FREQ=MONTHLY;BYMONTHDAY=-1");
        assert_eq!((last.freq, last.month_days.as_slice()), (RepeatFreq::Monthly, [-1].as_slice()));
    }

    #[test]
    fn yearly_with_interval_month_and_day() {
        let plain = round_trip("FREQ=YEARLY");
        assert_eq!((plain.freq, plain.months.len(), plain.month_days.len()), (RepeatFreq::Yearly, 0, 0));
        let every_other = round_trip("FREQ=YEARLY;INTERVAL=2;BYMONTH=10;BYMONTHDAY=13");
        assert_eq!((every_other.interval, every_other.months.as_slice()), (2, [10].as_slice()));
        assert_eq!(every_other.month_days, [13]);
    }

    #[test]
    fn count_and_until_with_a_day_or_an_instant() {
        assert_eq!(round_trip("FREQ=DAILY;COUNT=9").end, RepeatEnd::Count(9));
        let by_day = round_trip("FREQ=WEEKLY;BYDAY=MO;UNTIL=20261231");
        assert_eq!(by_day.end, RepeatEnd::Until(EventTime::Date(NaiveDate::from_ymd_opt(2026, 12, 31).expect("day"))));
        let by_time = round_trip("FREQ=WEEKLY;BYDAY=MO;UNTIL=20261230T225959Z");
        let at = NaiveDate::from_ymd_opt(2026, 12, 30).and_then(|d| d.and_hms_opt(22, 59, 59)).expect("time").and_utc();
        assert_eq!(by_time.end, RepeatEnd::Until(EventTime::DateTime(at)));
        // Without Z, in the zone of the start: 23:59:59 in Berlin in winter is 22:59:59 UTC.
        let floating = read("FREQ=WEEKLY;BYDAY=MO;UNTIL=20261230T235959", Some(Tz::Europe__Berlin));
        assert_eq!(floating.end, by_time.end);
        assert!(!floating.custom);
    }

    #[test]
    fn parts_the_fields_cannot_hold_make_the_rule_custom_and_keep_its_text() {
        for rule in [
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
            "FREQ=DAILY;BYHOUR=9,17",
            "FREQ=DAILY;BYMINUTE=30",
            "FREQ=YEARLY;BYWEEKNO=20;BYDAY=MO",
            "FREQ=YEARLY;BYYEARDAY=100",
            "FREQ=HOURLY;INTERVAL=4",
            "FREQ=WEEKLY;X-NAME=1",
            "BYDAY=MO",
            "FREQ=WEEKLY;INTERVAL=0",
            "FREQ=MONTHLY;BYMONTHDAY=32",
            "FREQ=WEEKLY;BYDAY=2MO",
            "FREQ=DAILY;COUNT=3;UNTIL=20261231",
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;WKST=SU",
        ] {
            let repeat = read(rule, None);
            assert!(repeat.custom, "{rule}");
            assert_eq!(repeat.rule, rule);
            let changed = Repeat { interval: 7, ..repeat.clone() };
            // A text without FREQ is no rule a client may write: nothing is written for it.
            let written = if check(rule).is_ok() { rule } else { "" };
            assert_eq!(write(&changed), written, "a custom rule is written back as it is: {rule}");
        }
        let set_pos = read("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1", None);
        assert_eq!((set_pos.freq, set_pos.weekdays.len()), (RepeatFreq::Monthly, 5), "read as far as it goes");
        // Apple writes WKST=SU everywhere; where it changes nothing, the rule stays plain.
        assert!(!read("FREQ=WEEKLY;BYDAY=MO,WE;WKST=SU", None).custom);
        assert!(!read("freq=weekly;byday=tu", None).custom, "names in any case");
    }

    #[test]
    fn only_one_line_of_the_rrule_grammar_passes_the_check() {
        for good in [
            "",
            "FREQ=WEEKLY;BYDAY=TU",
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
            "freq=daily;until=20261231T225959Z",
            "FREQ=YEARLY;BYDAY=+4TU;X-NAME=1",
            "FREQ=WEEKLY; BYDAY=TU",
        ] {
            assert_eq!(check(good), Ok(()), "{good}");
        }
        for bad in [
            "FREQ=WEEKLY\r\nATTENDEE:mailto:eve@example.org",
            "FREQ=WEEKLY\nEND:VEVENT",
            "FREQ=WEEKLY;BYDAY=TU\u{0}",
            "FREQ=WEEKLY;X-URL=https://example.org",
            "FREQ=WEEKLY;BYDAY",
            "FREQ=WEEKLY;=TU",
            "FREQ=WEEKLY;BYDAY=TU=WE",
            "BYDAY=TU",
            "FREQ=WEEKLY;FREQ=DAILY",
            "FREQ=FORTNIGHTLY",
            "FREQ=WEEKLY;BYDAY=TU\u{2028}",
            "FREQ=WEEKLY;BYDAY=TU\r\n",
        ] {
            assert!(check(bad).is_err(), "{bad:?}");
        }
        assert!(check(&format!("FREQ=DAILY;{}", "X-A=1;".repeat(200))).is_err(), "too long");
    }

    #[test]
    fn a_changed_rule_is_written_from_the_fields() {
        let stored = read("FREQ=WEEKLY;WKST=SU;BYDAY=TU", None);
        assert_eq!(write(&stored), "FREQ=WEEKLY;WKST=SU;BYDAY=TU", "untouched: the stored text");
        let changed = Repeat { weekdays: vec![day(Weekday::Thursday)], end: RepeatEnd::Count(5), ..stored };
        assert_eq!(write(&changed), "FREQ=WEEKLY;BYDAY=TH;COUNT=5");
        let new = Repeat { rule: String::new(), interval: 0, ..read("FREQ=DAILY", None) };
        assert_eq!(write(&new), "FREQ=DAILY", "an interval of 0 is every day");
    }
}
