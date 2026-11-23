//! The Areas view: how long each area has gone without an action, from its
//! log. Port of Cerqua's `lib/areaActions.ts` (1.46.0), same thresholds.

use crate::stamps::alive;
use balancecraft_model::{Area, AreaAction};
use chrono::DateTime;
use std::cmp::Ordering;

const MS_PER_DAY: i64 = 86_400_000;
const SILENCE_WARNING_DAYS: i64 = 14;
const SILENCE_DANGER_DAYS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Normal,
    Warning,
    Danger,
}

/// Most recent live log entry of the area, by `at`.
pub fn last_action_for<'a>(area_id: &str, actions: &'a [AreaAction]) -> Option<&'a AreaAction> {
    actions.iter().filter(|e| e.area_id == area_id && alive(e.meta.as_ref())).fold(
        None,
        |last: Option<&AreaAction>, e| match last {
            Some(l) if e.at <= l.at => Some(l),
            _ => Some(e),
        },
    )
}

fn ms(iso: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(iso).ok().map(|d| d.timestamp_millis())
}

/// Whole days since the last action; `None` when the area never had one.
pub fn silence_days(area_id: &str, actions: &[AreaAction], now_ms: i64) -> Option<i64> {
    let last = last_action_for(area_id, actions)?;
    // An unreadable date counts as "today", as Cerqua's NaN does after max(0).
    let days = ms(&last.at).map_or(0, |at| (now_ms - at).div_euclid(MS_PER_DAY));
    Some(days.max(0))
}

/// Never touched is the worst case.
pub fn severity(silence: Option<i64>) -> Severity {
    match silence {
        None => Severity::Danger,
        Some(d) if d > SILENCE_DANGER_DAYS => Severity::Danger,
        Some(d) if d > SILENCE_WARNING_DAYS => Severity::Warning,
        Some(_) => Severity::Normal,
    }
}

pub struct Row<'a> {
    pub area: &'a Area,
    pub last: Option<&'a AreaAction>,
    pub silence: Option<i64>,
}

/// One row per visible area, longest silence first (never touched on top),
/// then by name.
pub fn rows<'a>(areas: &'a [Area], actions: &'a [AreaAction], now_ms: i64) -> Vec<Row<'a>> {
    let mut rows: Vec<Row<'a>> = areas
        .iter()
        .filter(|a| alive(a.meta.as_ref()))
        .map(|area| Row {
            area,
            last: last_action_for(&area.id, actions),
            silence: silence_days(&area.id, actions, now_ms),
        })
        .collect();
    rows.sort_by(|x, y| {
        let sx = x.silence.unwrap_or(i64::MAX);
        let sy = y.silence.unwrap_or(i64::MAX);
        sy.cmp(&sx).then_with(|| x.area.name.to_lowercase().cmp(&y.area.name.to_lowercase()))
    });
    rows
}

/// Live log entries of the area, most recent first (ties by id, descending).
pub fn actions_for<'a>(area_id: &str, actions: &'a [AreaAction]) -> Vec<&'a AreaAction> {
    let mut out: Vec<&AreaAction> = actions.iter().filter(|e| e.area_id == area_id && alive(e.meta.as_ref())).collect();
    out.sort_by(|a, b| match b.at.cmp(&a.at) {
        Ordering::Equal => b.id.cmp(&a.id),
        o => o,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &str, area: &str, at: &str) -> AreaAction {
        AreaAction { id: id.into(), area_id: area.into(), at: at.into(), ..AreaAction::default() }
    }

    #[test]
    fn silence_and_order() {
        let now = ms("2026-11-23T12:00:00.000Z").unwrap_or_default();
        let areas: Vec<Area> = ["Work", "Home", "Body"]
            .iter()
            .map(|n| Area { id: n.to_lowercase(), name: (*n).into(), ..Area::default() })
            .collect();
        let actions = vec![
            action("1", "work", "2026-11-20T12:00:00.000Z"),
            action("2", "work", "2026-11-22T13:00:00.000Z"),
            action("3", "home", "2026-10-01T12:00:00.000Z"),
        ];
        assert_eq!(silence_days("work", &actions, now), Some(0));
        assert_eq!(silence_days("home", &actions, now), Some(53));
        assert_eq!(silence_days("body", &actions, now), None);
        let names: Vec<&str> = rows(&areas, &actions, now).iter().map(|r| r.area.name.as_str()).collect();
        assert_eq!(names, ["Body", "Home", "Work"]);
        assert_eq!(severity(Some(15)), Severity::Warning);
        assert_eq!(severity(Some(14)), Severity::Normal);
        let ids: Vec<&str> = actions_for("work", &actions).iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["2", "1"]);
    }
}
