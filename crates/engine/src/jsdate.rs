//! Date parsing with the rules of JavaScript's `new Date(string)` for the ISO
//! forms Cerqua writes, so that the algorithm gives the same numbers.
//!
//! - `YYYY-MM-DD`: midnight UTC.
//! - `YYYY-MM-DDTHH:MM[:SS[.sss]]` without an offset: local time.
//! - The same with `Z` or `±HH:MM`: that instant.
//!
//! Anything else is not a date (`None`, JavaScript's `NaN`).

use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone};

/// Milliseconds since the epoch of a local wall-clock time, resolved as
/// JavaScript does: the earlier instant when the time occurs twice (autumn
/// change), and the time moved forward by the gap when it does not exist
/// (spring change).
pub(crate) fn local_ms<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> Option<i64> {
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(t) => Some(t.timestamp_millis()),
        LocalResult::Ambiguous(a, b) => Some(a.timestamp_millis().min(b.timestamp_millis())),
        LocalResult::None => {
            // The offset before the gap applies: equivalent to moving forward by the gap.
            let before = tz.from_local_datetime(&(naive - Duration::hours(3))).earliest()?;
            let offset_ms = before.naive_local().signed_duration_since(before.naive_utc()).num_milliseconds();
            Some(naive.and_utc().timestamp_millis() - offset_ms)
        }
    }
}

pub(crate) fn parse<Tz: TimeZone>(tz: &Tz, s: &str) -> Option<i64> {
    if s.len() == 10 {
        let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
        return Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
    }
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some(t.timestamp_millis());
    }
    for f in ["%Y-%m-%dT%H:%MZ", "%Y-%m-%dT%H:%M%:z"] {
        if let Ok(t) = DateTime::parse_from_str(s, f) {
            return Some(t.timestamp_millis());
        }
        if f.ends_with('Z')
            && let Ok(n) = NaiveDateTime::parse_from_str(s, f)
        {
            return Some(n.and_utc().timestamp_millis());
        }
    }
    for f in ["%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M:%S%.f"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, f) {
            return local_ms(tz, n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Rome;

    #[test]
    fn parses_like_javascript() {
        assert_eq!(parse(&Rome, "2026-01-10"), Some(1_768_003_200_000));
        assert_eq!(parse(&Rome, "2026-01-10T10:00"), Some(1_768_035_600_000));
        assert_eq!(parse(&Rome, "2026-01-10T09:00:00.000Z"), Some(1_768_035_600_000));
        assert_eq!(parse(&Rome, "2026-01-10T10:00:00+01:00"), Some(1_768_035_600_000));
        assert_eq!(parse(&Rome, ""), None);
        assert_eq!(parse(&Rome, "domani"), None);
    }

    #[test]
    fn resolves_daylight_saving_changes_like_javascript() {
        // 29/3/2026 02:30 does not exist in Rome: JavaScript gives 03:30 CEST = 01:30Z.
        assert_eq!(parse(&Rome, "2026-03-29T02:30"), parse(&Rome, "2026-03-29T01:30:00Z"));
        // 25/10/2026 02:30 happens twice: JavaScript takes the first, CEST = 00:30Z.
        assert_eq!(parse(&Rome, "2026-10-25T02:30"), parse(&Rome, "2026-10-25T00:30:00Z"));
    }
}
