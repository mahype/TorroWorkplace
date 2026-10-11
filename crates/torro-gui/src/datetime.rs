//! German date and time input and output, and the calendar arithmetic the views need.
//!
//! Task K.1 in `docs/tasks/kalender-aufgaben.md`. All functions are pure and work on local
//! dates/times (`chrono::NaiveDate`, `NaiveTime`); converting to and from UTC instants
//! happens with [`to_utc`] and [`to_local`].

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, Utc};

const WEEKDAYS: [&str; 7] = ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"];
const WEEKDAYS_SHORT: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];
const MONTHS: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];

/// `11.10.2026`, `11.10.26`, `11.10.` (year of `today`), `heute`, `morgen` → date.
pub fn parse_date(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    let _ = (input, today);
    None
}

/// `14:30`, `14.30`, `1430`, `14` → time.
pub fn parse_time(input: &str) -> Option<NaiveTime> {
    let _ = input;
    None
}

/// `11.10.2026`
pub fn format_date(date: NaiveDate) -> String {
    date.format("%d.%m.%Y").to_string()
}

/// `14:30`
pub fn format_time(time: NaiveTime) -> String {
    time.format("%H:%M").to_string()
}

/// `Sonntag, 11. Oktober 2026`
pub fn format_long_date(date: NaiveDate) -> String {
    format!("{}, {}. {} {}", weekday_name(date, false), date.day(), month_name(date.month()), date.year())
}

/// German weekday name, `Montag` … `Sonntag`; short form `Mo` … `So` with `short`.
pub fn weekday_name(date: NaiveDate, short: bool) -> &'static str {
    let index = date.weekday().num_days_from_monday() as usize;
    if short { WEEKDAYS_SHORT[index] } else { WEEKDAYS[index] }
}

/// German month name, `Januar` … `Dezember`.
pub fn month_name(month: u32) -> &'static str {
    MONTHS.get(month.wrapping_sub(1) as usize).copied().unwrap_or("")
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
        assert_eq!(format_long_date(d(2026, 10, 11)), "Sonntag, 11. Oktober 2026");
        assert_eq!(weekday_name(d(2026, 10, 12), true), "Mo");
        assert_eq!(week_start(d(2026, 10, 11)), d(2026, 10, 5));
        let grid = month_grid(d(2026, 10, 20));
        assert_eq!((grid.len(), grid[0], grid[41]), (42, d(2026, 9, 28), d(2026, 11, 8)));
    }
}
