//! Deadline light and Italian working days. Port of Cerqua's `deadline.ts` and
//! `workdays.ts` (1.46.0).
//!
//! Not working days: Saturday, Sunday and the Italian national holidays, Easter
//! Monday included; the local patron saint is not handled. Dates are
//! "YYYY-MM-DD" strings; the time of a "YYYY-MM-DDTHH:MM" deadline is ignored.

use chrono::{Datelike, NaiveDate, Weekday};

const FIXED_HOLIDAYS: [(u32, u32); 10] =
    [(1, 1), (1, 6), (4, 25), (5, 1), (6, 2), (8, 15), (11, 1), (12, 8), (12, 25), (12, 26)];

/// Saint Francis of Assisi, a national holiday from 2026 (law 151 of 8 October 2025).
const SAINT_FRANCIS: (u32, u32) = (10, 4);
const SAINT_FRANCIS_FROM: i32 = 2026;

fn date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

/// Easter Sunday in the Gregorian calendar (Meeus/Jones/Butcher).
pub fn easter_sunday(year: i32) -> Option<NaiveDate> {
    let a = year.rem_euclid(19);
    let b = year.div_euclid(100);
    let c = year.rem_euclid(100);
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15).rem_euclid(30);
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k).rem_euclid(7);
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    NaiveDate::from_ymd_opt(year, u32::try_from(month).ok()?, u32::try_from(day).ok()?)
}

fn is_holiday(d: NaiveDate) -> bool {
    let md = (d.month(), d.day());
    FIXED_HOLIDAYS.contains(&md)
        || (d.year() >= SAINT_FRANCIS_FROM && md == SAINT_FRANCIS)
        || easter_sunday(d.year()).and_then(|e| e.succ_opt()) == Some(d)
}

fn is_weekend(d: NaiveDate) -> bool {
    matches!(d.weekday(), Weekday::Sat | Weekday::Sun)
}

pub fn is_working_day(s: &str) -> bool {
    date(s).is_some_and(|d| !is_weekend(d) && !is_holiday(d))
}

/// Working days in (from, to]: today excluded, deadline included; 0 when
/// `to` is not after `from` or a date is not valid. Friday to Monday is 1.
pub fn working_days_between(from: &str, to: &str) -> i64 {
    let (Some(start), Some(end)) = (date(from), date(to)) else { return 0 };
    if end <= start {
        return 0;
    }
    let span = (end - start).num_days();
    let weeks = span / 7;
    let mut count = weeks * 5;
    let mut d = start + chrono::Duration::days(weeks * 7);
    while d < end {
        d = d.succ_opt().unwrap_or(end);
        if !is_weekend(d) {
            count += 1;
        }
    }
    for year in start.year()..=end.year() {
        let mut days: Vec<NaiveDate> =
            FIXED_HOLIDAYS.iter().filter_map(|&(m, dd)| NaiveDate::from_ymd_opt(year, m, dd)).collect();
        if year >= SAINT_FRANCIS_FROM {
            days.extend(NaiveDate::from_ymd_opt(year, SAINT_FRANCIS.0, SAINT_FRANCIS.1));
        }
        days.extend(easter_sunday(year).and_then(|e| e.succ_opt()));
        count -=
            i64::try_from(days.iter().filter(|h| **h > start && **h <= end && !is_weekend(**h)).count()).unwrap_or(0);
    }
    count
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    Red,
    Yellow,
    Green,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Overdue,
    Today,
    Future,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadlineStatus {
    pub light: Light,
    pub when: When,
    /// Working days to the deadline; 0 when overdue or due today.
    pub working_days_left: i64,
}

/// Red: overdue or due today. Yellow: at most `advance_days` working days left.
/// Green: further. Independent of Today's ranking, which counts calendar days.
pub fn deadline_status(due_date: &str, today: &str, advance_days: i64) -> DeadlineStatus {
    let due = due_date.get(..10).unwrap_or(due_date);
    if due < today {
        return DeadlineStatus { light: Light::Red, when: When::Overdue, working_days_left: 0 };
    }
    if due == today {
        return DeadlineStatus { light: Light::Red, when: When::Today, working_days_left: 0 };
    }
    let left = working_days_between(today, due);
    DeadlineStatus {
        light: if left <= advance_days { Light::Yellow } else { Light::Green },
        when: When::Future,
        working_days_left: left,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easter_dates() {
        assert_eq!(easter_sunday(2026), NaiveDate::from_ymd_opt(2026, 4, 5));
        assert_eq!(easter_sunday(2027), NaiveDate::from_ymd_opt(2027, 3, 28));
        assert_eq!(easter_sunday(2000), NaiveDate::from_ymd_opt(2000, 4, 23));
    }

    #[test]
    fn working_days() {
        // Friday 2026-11-20 → Monday 2026-11-23.
        assert_eq!(working_days_between("2026-11-20", "2026-11-23"), 1);
        assert_eq!(working_days_between("2026-11-23", "2026-11-20"), 0);
        // Easter Monday 2026-04-06 and 25 April (a Saturday) in the range.
        assert_eq!(working_days_between("2026-04-03", "2026-04-10"), 4);
        // Saint Francis 2026-10-04 is a Sunday; in 2027 a Monday.
        assert_eq!(working_days_between("2027-10-01", "2027-10-05"), 1);
        assert_eq!(working_days_between("2026-12-23", "2027-01-08"), 9);
        assert!(!is_working_day("2026-12-08"));
        assert!(is_working_day("2026-12-09"));
        assert_eq!(working_days_between("x", "2026-01-01"), 0);
    }

    #[test]
    fn lights() {
        let s = deadline_status("2026-11-20T10:00", "2026-11-23", 5);
        assert_eq!((s.light, s.when), (Light::Red, When::Overdue));
        assert_eq!(deadline_status("2026-11-23", "2026-11-23", 5).when, When::Today);
        let s = deadline_status("2026-11-27", "2026-11-23", 5);
        assert_eq!((s.light, s.working_days_left), (Light::Yellow, 4));
        assert_eq!(deadline_status("2026-12-15", "2026-11-23", 5).light, Light::Green);
        assert_eq!(deadline_status("2026-11-24", "2026-11-23", 0).light, Light::Green);
    }
}
