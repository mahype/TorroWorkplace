//! The time zone of an event in the editor (task TE.7, `docs/termin-editor.md` decision 7): reading
//! and writing its times in that zone, the name the "Time zone" row shows ("Berlin  CET · your time
//! zone") and the search of the zone menu over the IANA names `chrono-tz` knows.
//!
//! The form shows an event's times on the wall clock of its zone, as Apple Calendar does; without a
//! zone of its own (`Event::time_zone` is `None`) that is this computer's zone, as everywhere else in
//! the app. Choosing another zone keeps the wall clock ("10:00") and moves the instant, so a meeting
//! set up for New York stays at 10:00 there.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::{OffsetName, TZ_VARIANTS, Tz};
use torro_gui::datetime::{to_local, to_utc};

/// The IANA name of this computer's zone, when the system names one `chrono-tz` knows.
pub(super) fn machine() -> Option<String> {
    iana_time_zone::get_timezone().ok().filter(|name| name.parse::<Tz>().is_ok())
}

/// The zone `name` names, if `chrono-tz` knows it.
fn tz(name: Option<&str>) -> Option<Tz> {
    name.and_then(|name| name.parse::<Tz>().ok())
}

/// The wall clock of `at` in the zone `zone`; on this computer's clock without a zone it knows.
pub(super) fn wall(at: DateTime<Utc>, zone: Option<&str>) -> NaiveDateTime {
    match tz(zone) {
        Some(tz) => at.with_timezone(&tz).naive_local(),
        None => to_local(at),
    }
}

/// The instant of the wall clock `wall` in the zone `zone`; on this computer's clock without a zone it
/// knows. A wall clock that falls into the hour skipped in spring is read after the change, one that
/// comes twice in autumn as the first.
pub(super) fn instant(wall: NaiveDateTime, zone: Option<&str>) -> DateTime<Utc> {
    let Some(tz) = tz(zone) else { return to_utc(wall) };
    match tz.from_local_datetime(&wall).earliest() {
        Some(at) => at.with_timezone(&Utc),
        // In the gap: an hour later is past it.
        None => tz
            .from_local_datetime(&(wall + chrono::Duration::hours(1)))
            .earliest()
            .map_or_else(|| to_utc(wall), |at| at.with_timezone(&Utc)),
    }
}

/// The city of a zone name, as people say it: "New York" for `America/New_York`.
pub(super) fn city(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).replace('_', " ")
}

/// A zone name to read: `America/New York`.
pub(super) fn readable(name: &str) -> String {
    name.replace('_', " ")
}

/// The abbreviation of the zone `name` at `at` ("CET", "CEST", "EDT"); `None` for a zone `chrono-tz`
/// does not know or that has none.
pub(super) fn abbreviation(name: &str, at: DateTime<Utc>) -> Option<String> {
    let tz = tz(Some(name))?;
    tz.offset_from_utc_datetime(&at.naive_utc()).abbreviation().map(str::to_string)
}

/// The zones whose name contains every word of `query` (any case, a space matching `_` and `/`), in
/// alphabetical order, at most `limit`; none for an empty query.
pub(super) fn search(query: &str, limit: usize) -> Vec<&'static str> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<&'static str> = TZ_VARIANTS
        .iter()
        .map(|tz| tz.name())
        .filter(|name| {
            let name = name.to_lowercase().replace('_', " ");
            words.iter().all(|word| name.contains(word.as_str()))
        })
        .collect();
    found.sort_unstable();
    found.truncate(limit);
    found
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn wall_at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).and_then(|day| day.and_hms_opt(h, min, 0)).expect("a time")
    }

    #[test]
    fn times_are_read_and_written_on_the_wall_clock_of_their_zone() {
        let berlin = Some("Europe/Berlin");
        // Summer and winter time: 10:00 in Berlin is 8:00 and 9:00 UTC.
        let summer = instant(wall_at(2026, 10, 20, 10, 0), berlin);
        assert_eq!(summer, wall_at(2026, 10, 20, 8, 0).and_utc());
        let winter = instant(wall_at(2026, 10, 27, 10, 0), berlin);
        assert_eq!(winter, wall_at(2026, 10, 27, 9, 0).and_utc());
        assert_eq!(wall(winter, berlin), wall_at(2026, 10, 27, 10, 0));
        assert_eq!(wall(winter, Some("America/New_York")), wall_at(2026, 10, 27, 5, 0));
        // The hour skipped in spring is read after the change.
        let gap = instant(wall_at(2026, 3, 29, 2, 30), berlin);
        assert_eq!(gap, wall_at(2026, 3, 29, 1, 30).and_utc());
        // Without a known zone: this computer's clock, as the rest of the app.
        assert_eq!(instant(wall_at(2026, 10, 27, 10, 0), None), to_utc(wall_at(2026, 10, 27, 10, 0)));
        assert_eq!(wall(winter, Some("W. Europe Standard Time")), to_local(winter));
    }

    #[test]
    fn a_zone_reads_as_its_city_with_its_abbreviation() {
        assert_eq!(city("America/New_York"), "New York");
        assert_eq!(city("UTC"), "UTC");
        assert_eq!(readable("America/Argentina/Buenos_Aires"), "America/Argentina/Buenos Aires");
        let winter = wall_at(2026, 12, 1, 12, 0).and_utc();
        assert_eq!(abbreviation("Europe/Berlin", winter).as_deref(), Some("CET"));
        assert_eq!(abbreviation("Nowhere/Else", winter), None);
    }

    #[test]
    fn the_search_finds_zones_by_any_part_of_their_name() {
        assert_eq!(search("new york", 10), ["America/New_York"]);
        assert!(search("berlin", 10).contains(&"Europe/Berlin"));
        assert!(search("EUROPE", 5).len() == 5, "at most the limit");
        assert!(search("  ", 10).is_empty());
        assert!(search("atlantis", 10).is_empty());
    }
}
