//! Sizes, counts and durations as people read them: one size format for every page ("60.9 MB",
//! "2.36 GB"), counts with the thousands separator of the language ("4,796", German "4.796") and
//! how long something took ("0.8 s", "2 min 14 s", "1 h 3 min", issue #422).
//!
//! Task DS.2 in `docs/tasks/detailseiten.md`. Sizes use base 1024 with the units B, KB, MB, GB
//! and TB, so an IMAP quota (reported in KiB) of 10485760 shows as "10 GB", as the provider sells
//! it. Three significant digits are enough to compare sizes; more would only suggest a precision
//! the numbers do not have.

use torro_core::i18n::{Lang, lang};

const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

/// `0 B`, `1023 B`, `1 KB`, `60.9 MB`, `512 MB`, `2.36 GB`, `10 GB`; German with a decimal comma
/// (`60,9 MB`).
pub fn format_bytes(bytes: u64) -> String {
    format_bytes_in(lang(), bytes)
}

/// `4,796` (German `4.796`).
pub fn format_count(count: u64) -> String {
    format_count_in(lang(), count)
}

/// How long something took, from milliseconds: `0.8 s` (one decimal below ten seconds), `42 s`,
/// `2 min 14 s`, `1 h 3 min`; German with a decimal comma (`0,8 s`). Two units at most: next to a
/// minute the tenths of a second, next to an hour the seconds say nothing. The units are the
/// international symbols, the same in every language.
pub fn format_duration(ms: u64) -> String {
    format_duration_in(lang(), ms)
}

/// [`format_duration`] in a given language, so tests can check both.
pub fn format_duration_in(lang: Lang, ms: u64) -> String {
    let tenths = ms.saturating_add(50) / 100;
    if tenths < 100 {
        let (whole, tenth) = (tenths / 10, tenths % 10);
        if tenth == 0 {
            return format!("{whole} s");
        }
        let comma = match lang {
            Lang::En => '.',
            Lang::De => ',',
        };
        return format!("{whole}{comma}{tenth} s");
    }
    let seconds = ms.saturating_add(500) / 1000;
    if seconds < 60 {
        return format!("{seconds} s");
    }
    if seconds < 3600 {
        let (minutes, rest) = (seconds / 60, seconds % 60);
        return if rest == 0 { format!("{minutes} min") } else { format!("{minutes} min {rest} s") };
    }
    let minutes = ms.saturating_add(30_000) / 60_000;
    let (hours, rest) = (minutes / 60, minutes % 60);
    if rest == 0 { format!("{hours} h") } else { format!("{hours} h {rest} min") }
}

/// [`format_bytes`] in a given language, so tests can check both.
pub fn format_bytes_in(lang: Lang, bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} {}", UNITS[0]);
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let mut digits = significant(value);
    // 1023.9 KB rounds to "1024 KB"; that is 1 MB.
    if digits == "1024" && unit < UNITS.len() - 1 {
        unit += 1;
        digits = "1".to_string();
    }
    let digits = match lang {
        Lang::En => digits,
        Lang::De => digits.replace('.', ","),
    };
    format!("{digits} {}", UNITS[unit])
}

/// [`format_count`] in a given language, so tests can check both.
pub fn format_count_in(lang: Lang, count: u64) -> String {
    let separator = match lang {
        Lang::En => ',',
        Lang::De => '.',
    };
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(digit);
    }
    out
}

/// `value` (1 ≤ value < 1024) with three significant digits and without trailing zeros: 60.9,
/// 512, 2.36, 10. Values of 1000 and more keep all four whole digits.
fn significant(value: f64) -> String {
    let decimals = match value {
        v if v >= 99.95 => 0,
        v if v >= 9.995 => 1,
        _ => 2,
    };
    let text = format!("{value:.decimals$}");
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;

    #[test]
    fn sizes_have_three_significant_digits() {
        let en = |bytes| format_bytes_in(Lang::En, bytes);
        assert_eq!(en(0), "0 B");
        assert_eq!(en(1023), "1023 B");
        assert_eq!(en(KIB), "1 KB");
        assert_eq!(en(60 * MIB + 921 * KIB), "60.9 MB");
        assert_eq!(en(512 * MIB), "512 MB");
        assert_eq!(en(2 * GIB + 369 * MIB), "2.36 GB");
        assert_eq!(en(1536 * GIB), "1.5 TB");
    }

    #[test]
    fn a_quota_in_kib_shows_as_the_provider_sells_it() {
        assert_eq!(format_bytes_in(Lang::En, 10_485_760 * KIB), "10 GB");
    }

    #[test]
    fn rounding_up_to_1024_moves_to_the_next_unit() {
        assert_eq!(format_bytes_in(Lang::En, MIB - 1), "1 MB");
        assert_eq!(format_bytes_in(Lang::En, 1000 * KIB), "1000 KB");
        assert_eq!(format_bytes_in(Lang::En, 102_389), "100 KB");
        assert_eq!(format_bytes_in(Lang::En, 10_236), "10 KB");
    }

    #[test]
    fn german_sizes_use_a_decimal_comma() {
        assert_eq!(format_bytes_in(Lang::De, 60 * MIB + 921 * KIB), "60,9 MB");
        assert_eq!(format_bytes_in(Lang::De, 2 * GIB + 369 * MIB), "2,36 GB");
        assert_eq!(format_bytes_in(Lang::De, 512 * MIB), "512 MB");
    }

    #[test]
    fn durations_use_at_most_two_units() {
        let en = |ms| format_duration_in(Lang::En, ms);
        assert_eq!(en(0), "0 s");
        assert_eq!(en(800), "0.8 s");
        assert_eq!(en(2_000), "2 s");
        assert_eq!(en(9_940), "9.9 s");
        assert_eq!(en(9_960), "10 s", "ten seconds lose their tenth");
        assert_eq!(en(42_300), "42 s");
        assert_eq!(en(59_600), "1 min", "rounds up into the next minute");
        assert_eq!(en(134_000), "2 min 14 s");
        assert_eq!(en(3_780_000), "1 h 3 min");
        assert_eq!(en(7_200_000), "2 h");
    }

    #[test]
    fn german_durations_use_a_decimal_comma() {
        assert_eq!(format_duration_in(Lang::De, 800), "0,8 s");
        assert_eq!(format_duration_in(Lang::De, 134_000), "2 min 14 s");
    }

    #[test]
    fn counts_get_the_thousands_separator_of_the_language() {
        assert_eq!(format_count_in(Lang::En, 4796), "4,796");
        assert_eq!(format_count_in(Lang::De, 4796), "4.796");
        assert_eq!(format_count_in(Lang::En, 0), "0");
        assert_eq!(format_count_in(Lang::En, 999), "999");
        assert_eq!(format_count_in(Lang::En, 1_234_567), "1,234,567");
        assert_eq!(format_count_in(Lang::De, 100_000), "100.000");
    }
}
