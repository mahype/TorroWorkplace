//! German date and time input and output, and the calendar arithmetic the views need.
//!
//! One date form for the apps (D.7 in `docs/tasks/design.md`): the short form
//! [`format_short_date`] ("Sat, 3 Oct", German "Sa, 3. Okt.") wherever a date is shown in a row,
//! heading or chip; the long form [`format_long_date`] for titles; and [`format_date`]
//! (`11.10.2026`) for the fields people type into. [`parse_date`] stays tolerant: it reads the
//! field form, the short year, the day and month alone, and the keywords.
//!
//! [`format_moment`] names a past instant as people say it ("Today 15:39", "5 Oct 2020"), with
//! [`format_exact`] for its tooltip (DS.2 in `docs/tasks/detailseiten.md`).
//!
//! Task K.1 in `docs/tasks/kalender-aufgaben.md`. All functions are pure and work on local
//! dates/times (`chrono::NaiveDate`, `NaiveTime`); converting to and from UTC instants
//! happens with [`to_utc`] and [`to_local`].

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use torro_core::i18n::{tr, trf};

// English names; `tr` gives the German ones (see torro_core::i18n).
const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const WEEKDAYS_SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `11.10.2026`, `11.10.26`, `11.10.` (year of `today`), `11.10` → date. Keywords:
/// `heute` / `today`, `morgen` / `tomorrow`, `übermorgen` → today, tomorrow, the day after.
///
/// Keywords ignore case and surrounding spaces. Two-digit years mean 2000 + the year. Invalid
/// dates such as `31.02.` give `None`, not a clamped date.
pub fn parse_date(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    match input.trim().to_lowercase().as_str() {
        "heute" | "today" => return Some(today),
        "morgen" | "tomorrow" => return today.checked_add_signed(Duration::days(1)),
        "übermorgen" => return today.checked_add_signed(Duration::days(2)),
        _ => {}
    }

    let parts: Vec<&str> = input.trim().split('.').map(str::trim).collect();
    let (day, month, year) = match parts.as_slice() {
        [d, m] | [d, m, ""] => (number(d, 1, 2)?, number(m, 1, 2)?, today.year()),
        [d, m, y] => (number(d, 1, 2)?, number(m, 1, 2)?, parse_year(y)?),
        _ => return None,
    };
    NaiveDate::from_ymd_opt(year, month, day)
}

/// `14:30`, `14.30`, `1430`, `930`, `14`, `9` → time.
///
/// With a separator the minutes need two digits (`14:5` is rejected). Without one, the digits
/// are read as `H`, `HH`, `HMM` or `HHMM`. Hours above 23 and minutes above 59 give `None`.
pub fn parse_time(input: &str) -> Option<NaiveTime> {
    let text = input.trim();
    let (hour, minute) = if let Some((h, m)) = text.split_once(':').or_else(|| text.split_once('.')) {
        (number(h, 1, 2)?, number(m, 2, 2)?)
    } else {
        match text.len() {
            1 | 2 => (number(text, 1, 2)?, 0),
            3 => (number(text.get(..1)?, 1, 1)?, number(text.get(1..)?, 2, 2)?),
            4 => (number(text.get(..2)?, 2, 2)?, number(text.get(2..)?, 2, 2)?),
            _ => return None,
        }
    };
    NaiveTime::from_hms_opt(hour, minute, 0)
}

/// A string of `min..=max` ASCII digits as a number. Rejects signs and spaces inside the
/// digits, which `str::parse` would accept.
fn number(part: &str, min: usize, max: usize) -> Option<u32> {
    let len = part.len();
    if !(min..=max).contains(&len) || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

/// `26` → 2026, `2026` → 2026. Other lengths are not years.
fn parse_year(part: &str) -> Option<i32> {
    match part.len() {
        2 => number(part, 2, 2).map(|y| 2000 + y as i32),
        4 => number(part, 4, 4).map(|y| y as i32),
        _ => None,
    }
}

/// `11.10.2026`
pub fn format_date(date: NaiveDate) -> String {
    date.format("%d.%m.%Y").to_string()
}

/// `14:30`
pub fn format_time(time: NaiveTime) -> String {
    time.format("%H:%M").to_string()
}

/// `Sunday, 11 October 2026` (German: `Sonntag, 11. Oktober 2026`)
pub fn format_long_date(date: NaiveDate) -> String {
    trf(
        "{weekday}, {day} {month} {year}",
        &[
            ("weekday", &weekday_name(date, false)),
            ("day", &date.day()),
            ("month", &month_name(date.month())),
            ("year", &date.year()),
        ],
    )
}

/// `Sat, 3 Oct` (German `Sa, 3. Okt.`): the short form of a date, for rows, headings and chips.
/// A date outside the year of `today` gets its year (`Sat, 2 Jan 2027`), so it cannot be taken for
/// one of this year.
pub fn format_short_date(date: NaiveDate, today: NaiveDate) -> String {
    let weekday = weekday_name(date, true);
    let month = month_name_short(date.month());
    if date.year() == today.year() {
        trf("{weekday}, {day} {month}", &[("weekday", &weekday), ("day", &date.day()), ("month", &month)])
    } else {
        trf(
            "{weekday}, {day} {month} {year}",
            &[("weekday", &weekday), ("day", &date.day()), ("month", &month), ("year", &date.year())],
        )
    }
}

/// `3 Oct` (German `3. Okt.`): the short form without the weekday, where the weekday stands
/// beside it already or the room is too small for it.
pub fn format_day_month(date: NaiveDate) -> String {
    trf("{day} {month}", &[("day", &date.day()), ("month", &month_name_short(date.month()))])
}

/// `Today 15:39`, `Yesterday 09:12`, `5 Oct` in the year of `now`, otherwise `5 Oct 2020`: when
/// something happened, in local time. The exact value belongs in a tooltip ([`format_exact`]).
pub fn format_moment(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (at, now) = (to_local(at), to_local(now));
    let (day, today) = (at.date(), now.date());
    let time = format_time(at.time());
    if day == today {
        trf("Today {time}", &[("time", &time)])
    } else if today.pred_opt() == Some(day) {
        trf("Yesterday {time}", &[("time", &time)])
    } else if day.year() == today.year() {
        format_day_month(day)
    } else {
        trf(
            "{day} {month} {year}",
            &[("day", &day.day()), ("month", &month_name_short(day.month())), ("year", &day.year())],
        )
    }
}

/// `Monday, 5 October 2020, 15:39:12` in local time: the exact value behind [`format_moment`].
pub fn format_exact(at: DateTime<Utc>) -> String {
    let at = to_local(at);
    trf("{date}, {time}", &[("date", &format_long_date(at.date())), ("time", &at.time().format("%H:%M:%S"))])
}

/// Weekday name, `Monday` … `Sunday`; short form `Mon` … `Sun` with `short` (translated).
pub fn weekday_name(date: NaiveDate, short: bool) -> &'static str {
    let index = date.weekday().num_days_from_monday() as usize;
    tr(if short { WEEKDAYS_SHORT[index] } else { WEEKDAYS[index] })
}

/// Month name, `January` … `December` (translated).
pub fn month_name(month: u32) -> &'static str {
    MONTHS.get(month.wrapping_sub(1) as usize).map(|name| tr(name)).unwrap_or("")
}

/// Short month name, `Jan` … `Dec` (translated).
pub fn month_name_short(month: u32) -> &'static str {
    MONTHS_SHORT.get(month.wrapping_sub(1) as usize).map(|name| tr(name)).unwrap_or("")
}

/// The Monday of the ISO week containing `date`.
pub fn week_start(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

/// The 42 days (6 weeks, Monday first) the month view shows for the month of `date`.
pub fn month_grid(date: NaiveDate) -> Vec<NaiveDate> {
    let first = date.with_day(1).unwrap_or(date);
    let start = week_start(first);
    (0..42).map(|i| start + Duration::days(i)).collect()
}

/// A local date and time as a UTC instant (the earlier one in a DST fold, the next valid
/// one in a DST gap).
pub fn to_utc(local: NaiveDateTime) -> DateTime<Utc> {
    local.and_local_timezone(Local).earliest().map(|t| t.with_timezone(&Utc)).unwrap_or_else(|| local.and_utc())
}

/// A UTC instant in local time.
pub fn to_local(at: DateTime<Utc>) -> NaiveDateTime {
    at.with_timezone(&Local).naive_local()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn names_and_weeks() {
        let long = trf(
            "{weekday}, {day} {month} {year}",
            &[("weekday", &tr("Sunday")), ("day", &11), ("month", &tr("October")), ("year", &2026)],
        );
        assert_eq!(format_long_date(d(2026, 10, 11)), long);
        assert_eq!(weekday_name(d(2026, 10, 12), true), tr("Mon"));
        assert_eq!(week_start(d(2026, 10, 11)), d(2026, 10, 5));
        let grid = month_grid(d(2026, 10, 20));
        assert_eq!((grid.len(), grid[0], grid[41]), (42, d(2026, 9, 28), d(2026, 11, 8)));
    }

    #[test]
    fn the_short_form_names_weekday_day_and_month() {
        let today = d(2026, 10, 11);
        let expected = trf("{weekday}, {day} {month}", &[("weekday", &tr("Sat")), ("day", &3), ("month", &tr("Oct"))]);
        assert_eq!(format_short_date(d(2026, 10, 3), today), expected);
        assert_eq!(format_day_month(d(2026, 10, 3)), trf("{day} {month}", &[("day", &3), ("month", &tr("Oct"))]));
        assert_eq!(month_name_short(10), tr("Oct"));
        assert_eq!(month_name_short(13), "");
    }

    #[test]
    fn the_short_form_adds_the_year_for_other_years() {
        let today = d(2026, 10, 11);
        let expected = trf(
            "{weekday}, {day} {month} {year}",
            &[("weekday", &tr("Sat")), ("day", &2), ("month", &tr("Jan")), ("year", &2027)],
        );
        assert_eq!(format_short_date(d(2027, 1, 2), today), expected);
        assert_ne!(format_short_date(d(2027, 1, 2), today), format_short_date(d(2026, 1, 2), today));
    }

    #[test]
    fn what_the_fields_show_can_be_read_back() {
        let today = d(2026, 10, 11);
        for date in [d(2026, 10, 3), d(2027, 1, 2), d(2020, 2, 29)] {
            assert_eq!(parse_date(&format_date(date), today), Some(date));
        }
    }

    /// A local wall-clock time as the instant the functions get.
    fn local(y: i32, m: u32, day: u32, h: u32, min: u32) -> DateTime<Utc> {
        to_utc(d(y, m, day).and_hms_opt(h, min, 12).unwrap())
    }

    #[test]
    fn a_moment_says_today_yesterday_or_the_date() {
        let now = local(2026, 10, 26, 18, 0);
        assert_eq!(format_moment(local(2026, 10, 26, 15, 39), now), trf("Today {time}", &[("time", &"15:39")]));
        assert_eq!(format_moment(local(2026, 10, 25, 9, 12), now), trf("Yesterday {time}", &[("time", &"09:12")]));
        assert_eq!(format_moment(local(2026, 10, 5, 9, 12), now), format_day_month(d(2026, 10, 5)));
        let earlier = trf("{day} {month} {year}", &[("day", &5), ("month", &tr("Oct")), ("year", &2020)]);
        assert_eq!(format_moment(local(2020, 10, 5, 9, 12), now), earlier);
    }

    #[test]
    fn yesterday_crosses_the_year_end() {
        let now = local(2027, 1, 1, 0, 30);
        assert_eq!(format_moment(local(2026, 12, 31, 23, 0), now), trf("Yesterday {time}", &[("time", &"23:00")]));
        let older = trf("{day} {month} {year}", &[("day", &30), ("month", &tr("Dec")), ("year", &2026)]);
        assert_eq!(format_moment(local(2026, 12, 30, 23, 0), now), older);
    }

    #[test]
    fn the_exact_moment_names_weekday_date_and_seconds() {
        let expected = trf("{date}, {time}", &[("date", &format_long_date(d(2020, 10, 5))), ("time", &"15:39:12")]);
        assert_eq!(format_exact(local(2020, 10, 5, 15, 39)), expected);
    }

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn parses_full_and_short_dates() {
        let today = d(2026, 10, 11);
        assert_eq!(parse_date("11.10.2026", today), Some(d(2026, 10, 11)));
        assert_eq!(parse_date("11.10.26", today), Some(d(2026, 10, 11)));
        assert_eq!(parse_date("11.10.", today), Some(d(2026, 10, 11)));
        assert_eq!(parse_date("11.10", today), Some(d(2026, 10, 11)));
        assert_eq!(parse_date("1.2.", today), Some(d(2026, 2, 1)));
        assert_eq!(parse_date("01.02.2030", today), Some(d(2030, 2, 1)));
        assert_eq!(parse_date("  11.10.2026  ", today), Some(d(2026, 10, 11)));
        assert_eq!(parse_date("11. 10. 2026", today), Some(d(2026, 10, 11)));
    }

    #[test]
    fn two_digit_year_means_2000_plus() {
        let today = d(2026, 10, 11);
        assert_eq!(parse_date("1.1.99", today), Some(d(2099, 1, 1)));
        assert_eq!(parse_date("1.1.00", today), Some(d(2000, 1, 1)));
    }

    #[test]
    fn short_date_uses_the_year_of_today() {
        assert_eq!(parse_date("24.12.", d(2031, 6, 1)), Some(d(2031, 12, 24)));
    }

    #[test]
    fn parses_keywords_case_insensitively() {
        let today = d(2026, 10, 11);
        assert_eq!(parse_date("heute", today), Some(today));
        assert_eq!(parse_date("Heute", today), Some(today));
        assert_eq!(parse_date(" HEUTE ", today), Some(today));
        assert_eq!(parse_date("morgen", today), Some(d(2026, 10, 12)));
        assert_eq!(parse_date("Morgen", today), Some(d(2026, 10, 12)));
        assert_eq!(parse_date("übermorgen", today), Some(d(2026, 10, 13)));
        assert_eq!(parse_date("ÜBERMORGEN", today), Some(d(2026, 10, 13)));
    }

    #[test]
    fn parses_english_keywords() {
        let today = d(2026, 10, 11);
        assert_eq!(parse_date("today", today), Some(today));
        assert_eq!(parse_date(" Today ", today), Some(today));
        assert_eq!(parse_date("tomorrow", today), Some(d(2026, 10, 12)));
        assert_eq!(parse_date("TOMORROW", today), Some(d(2026, 10, 12)));
        assert_eq!(parse_date("tomorrow", d(2026, 12, 31)), Some(d(2027, 1, 1)));
    }

    #[test]
    fn keywords_cross_year_end() {
        let new_year_eve = d(2026, 12, 31);
        assert_eq!(parse_date("morgen", new_year_eve), Some(d(2027, 1, 1)));
        assert_eq!(parse_date("übermorgen", new_year_eve), Some(d(2027, 1, 2)));
    }

    #[test]
    fn rejects_invalid_dates() {
        let today = d(2026, 10, 11);
        assert_eq!(parse_date("31.02.", today), None);
        assert_eq!(parse_date("31.02.2026", today), None);
        assert_eq!(parse_date("29.02.2027", today), None);
        assert_eq!(parse_date("29.02.2028", today), Some(d(2028, 2, 29)));
        assert_eq!(parse_date("00.10.", today), None);
        assert_eq!(parse_date("11.13.", today), None);
        assert_eq!(parse_date("11.10.2026.5", today), None);
        assert_eq!(parse_date("11.10.202", today), None);
        assert_eq!(parse_date("11.10.20266", today), None);
        assert_eq!(parse_date("11.10.0", today), None);
        assert_eq!(parse_date("111.10.", today), None);
        assert_eq!(parse_date("+1.10.", today), None);
        assert_eq!(parse_date("11.10.-6", today), None);
        assert_eq!(parse_date("11.", today), None);
        assert_eq!(parse_date("11.10.2026.", today), None);
        assert_eq!(parse_date("11/10/2026", today), None);
        assert_eq!(parse_date("", today), None);
        assert_eq!(parse_date("gestern", today), None);
    }

    #[test]
    fn parses_times_with_separator() {
        assert_eq!(parse_time("14:30"), Some(t(14, 30)));
        assert_eq!(parse_time("14.30"), Some(t(14, 30)));
        assert_eq!(parse_time("9:05"), Some(t(9, 5)));
        assert_eq!(parse_time("09.05"), Some(t(9, 5)));
        assert_eq!(parse_time("0:00"), Some(t(0, 0)));
        assert_eq!(parse_time("23:59"), Some(t(23, 59)));
        assert_eq!(parse_time(" 14:30 "), Some(t(14, 30)));
    }

    #[test]
    fn parses_compact_and_hour_only_times() {
        assert_eq!(parse_time("1430"), Some(t(14, 30)));
        assert_eq!(parse_time("0930"), Some(t(9, 30)));
        assert_eq!(parse_time("930"), Some(t(9, 30)));
        assert_eq!(parse_time("025"), Some(t(0, 25)));
        assert_eq!(parse_time("14"), Some(t(14, 0)));
        assert_eq!(parse_time("9"), Some(t(9, 0)));
        assert_eq!(parse_time("0"), Some(t(0, 0)));
        assert_eq!(parse_time("23"), Some(t(23, 0)));
    }

    #[test]
    fn rejects_invalid_times() {
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("24"), None);
        assert_eq!(parse_time("2400"), None);
        assert_eq!(parse_time("14:60"), None);
        assert_eq!(parse_time("1460"), None);
        assert_eq!(parse_time("960"), None);
        assert_eq!(parse_time("14:5"), None);
        assert_eq!(parse_time("14:"), None);
        assert_eq!(parse_time(":30"), None);
        assert_eq!(parse_time("14:30:00"), None);
        assert_eq!(parse_time("14.30.00"), None);
        assert_eq!(parse_time("+9"), None);
        assert_eq!(parse_time("-1"), None);
        assert_eq!(parse_time("12345"), None);
        assert_eq!(parse_time("1 4"), None);
        assert_eq!(parse_time("é4"), None);
        assert_eq!(parse_time("ééé"), None);
        assert_eq!(parse_time(""), None);
        assert_eq!(parse_time("mittag"), None);
    }
}
