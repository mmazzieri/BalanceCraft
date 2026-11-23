//! Inherited settings: system default → global → area → task and its
//! ancestors, the closest value wins. Port of Cerqua's `settings.ts` (1.46.0).
//! Today the only setting is the warning before deadlines, in working days.

use balancecraft_model::{Area, Num, SettingValues, TaskNode};
use std::collections::HashMap;

pub const ADVANCE_DAYS_DEFAULT: i64 = 5;
pub const ADVANCE_DAYS_MIN: i64 = 0;
pub const ADVANCE_DAYS_MAX: i64 = 60;

/// A stored value counts only when it is a whole number in range: a file edited
/// by hand may hold anything, and then the value is inherited.
#[allow(clippy::cast_possible_truncation)]
pub fn valid_advance_days(v: Option<Num>) -> Option<i64> {
    let v = v?.get();
    (v.fract() == 0.0 && v >= ADVANCE_DAYS_MIN as f64 && v <= ADVANCE_DAYS_MAX as f64).then_some(v as i64)
}

/// Effective warning of every area and task, and the value it would have
/// without its own override (what a form shows as inherited).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub effective: i64,
    pub inherited: i64,
}

pub fn global_advance_days(global: Option<&SettingValues>) -> i64 {
    global.and_then(|g| valid_advance_days(g.advance_days)).unwrap_or(ADVANCE_DAYS_DEFAULT)
}

pub fn effective_settings(areas: &[Area], global: Option<&SettingValues>) -> HashMap<String, Resolved> {
    fn visit(node: &TaskNode, inherited: i64, map: &mut HashMap<String, Resolved>) {
        let effective = valid_advance_days(node.settings.advance_days).unwrap_or(inherited);
        map.insert(node.id.clone(), Resolved { effective, inherited });
        for c in &node.children {
            visit(c, effective, map);
        }
    }
    let base = global_advance_days(global);
    let mut map = HashMap::new();
    for a in areas {
        let effective = valid_advance_days(a.settings.advance_days).unwrap_or(base);
        map.insert(a.id.clone(), Resolved { effective, inherited: base });
        for c in &a.children {
            visit(c, effective, &mut map);
        }
    }
    map
}

/// The text of a settings field, parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parsed {
    /// Empty: the value is inherited.
    Inherit,
    Value(i64),
    /// Not a whole number in range.
    Invalid,
}

pub fn parse_advance_days(raw: &str) -> Parsed {
    let t = raw.trim();
    if t.is_empty() {
        return Parsed::Inherit;
    }
    match t.parse::<i64>() {
        Ok(v) if (ADVANCE_DAYS_MIN..=ADVANCE_DAYS_MAX).contains(&v) => Parsed::Value(v),
        _ => Parsed::Invalid,
    }
}
