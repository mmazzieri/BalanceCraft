//! The current time for the algorithm, in a given time zone.

use crate::jsdate;
use chrono::{DateTime, Duration, TimeZone};

/// Now, as the algorithm sees it: the instant and the local wall clock.
#[derive(Debug, Clone)]
pub struct Clock<Tz: TimeZone> {
    tz: Tz,
    now_ms: i64,
    now_dt: String,
}

impl<Tz: TimeZone> Clock<Tz> {
    /// The clock at `now_ms` (ms since the epoch) in `tz`.
    pub fn at(tz: Tz, now_ms: i64) -> Option<Self> {
        let now: DateTime<Tz> = tz.timestamp_millis_opt(now_ms).single()?;
        let now_dt = now.naive_local().format("%Y-%m-%dT%H:%M").to_string();
        Some(Clock { tz, now_ms, now_dt })
    }

    /// Instant, in ms since the epoch.
    pub fn now_ms(&self) -> i64 {
        self.now_ms
    }

    /// Local date-time "YYYY-MM-DDTHH:MM", compared with `startDate`.
    pub fn now_dt(&self) -> &str {
        &self.now_dt
    }

    /// A date as JavaScript's `new Date(s)` reads it, in ms since the epoch.
    pub fn parse(&self, s: &str) -> Option<i64> {
        jsdate::parse(&self.tz, s)
    }

    /// The same wall-clock time `days` calendar days earlier, in ms since the epoch.
    pub fn days_before(&self, days: i64) -> Option<i64> {
        let now = self.tz.timestamp_millis_opt(self.now_ms).single()?;
        jsdate::local_ms(&self.tz, now.naive_local() - Duration::days(days))
    }
}

impl Clock<chrono::Local> {
    /// The computer's clock and time zone.
    pub fn local() -> Option<Self> {
        Self::at(chrono::Local, chrono::Utc::now().timestamp_millis())
    }
}
