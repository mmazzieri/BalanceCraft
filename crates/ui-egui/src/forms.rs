//! Area and task forms, without the drawing: parsing of the fields and the
//! patch that Save writes.
//!
//! As in Cerqua since 1.44.2 (#154), an edit form compares the values at Save
//! with the ones it showed when it opened, and writes only the fields the user
//! changed: a field changed on another computer while the form was open is not
//! overwritten with the old value.

use crate::i18n::Strings;
use balancecraft_engine::commands::{AreaChange, NewArea, NewTask, TaskChange};
use balancecraft_engine::settings::{Parsed, parse_advance_days, valid_advance_days};
use balancecraft_model::{Area, Num, TaskNode};
use chrono::{NaiveDate, NaiveDateTime};

pub const DUE_TIME: &str = "23:59";
pub const START_TIME: &str = "00:00";
const WEIGHT_MAX: i64 = 100;

/// A date typed by the user, to the stored "YYYY-MM-DDTHH:MM". Empty is
/// `Ok(None)`; a date without a time takes `default_time`.
pub fn parse_date(raw: &str, default_time: &str) -> Result<Option<String>, ()> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if let Ok(d) = NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        return Ok(Some(format!("{}T{default_time}", d.format("%Y-%m-%d"))));
    }
    for fmt in ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(t, fmt) {
            return Ok(Some(dt.format("%Y-%m-%dT%H:%M").to_string()));
        }
    }
    Err(())
}

/// A stored date as the form shows it: the time only when it is not the
/// default one.
pub fn show_date(stored: Option<&str>, default_time: &str) -> String {
    let Some(s) = stored.filter(|s| !s.is_empty()) else { return String::new() };
    match s.split_once('T') {
        Some((d, t)) if t == default_time => d.to_owned(),
        Some((d, t)) => format!("{d} {t}"),
        None => s.to_owned(),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn show_num(v: Option<Num>) -> String {
    match v.map(Num::get) {
        Some(x) if x.fract() == 0.0 && x.is_finite() => format!("{}", x as i64),
        Some(x) => x.to_string(),
        None => String::new(),
    }
}

fn parse_weight(raw: &str) -> Result<Option<f64>, ()> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(None);
    }
    match t.parse::<i64>() {
        #[allow(clippy::cast_precision_loss)]
        Ok(v) if (1..=WEIGHT_MAX).contains(&v) => Ok(Some(v as f64)),
        _ => Err(()),
    }
}

#[allow(clippy::cast_precision_loss)]
fn parse_advance(raw: &str) -> Result<Option<f64>, ()> {
    match parse_advance_days(raw) {
        Parsed::Inherit => Ok(None),
        Parsed::Value(v) => Ok(Some(v as f64)),
        Parsed::Invalid => Err(()),
    }
}

/// The text of the fields of a task form.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskForm {
    pub name: String,
    pub weight: String,
    pub due: String,
    pub start: String,
    pub notes: String,
    pub advance: String,
}

/// A task form that passed validation.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskValues {
    pub name: String,
    pub weight: Option<f64>,
    pub due: Option<String>,
    pub start: Option<String>,
    pub notes: Option<String>,
    pub advance: Option<f64>,
}

impl TaskForm {
    pub fn new() -> Self {
        TaskForm { weight: "5".to_owned(), ..TaskForm::default() }
    }

    pub fn from_task(t: &TaskNode) -> Self {
        TaskForm {
            name: t.name.clone(),
            weight: show_num(t.weight),
            due: show_date(t.due_date.as_deref(), DUE_TIME),
            start: show_date(t.start_date.as_deref(), START_TIME),
            notes: t.notes.clone().unwrap_or_default(),
            advance: valid_advance_days(t.settings.advance_days).map(|v| v.to_string()).unwrap_or_default(),
        }
    }

    pub fn validate(&self, s: &Strings) -> Result<TaskValues, &'static str> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(s.err_name);
        }
        let notes = self.notes.trim_end();
        Ok(TaskValues {
            name: name.to_owned(),
            weight: parse_weight(&self.weight).map_err(|()| s.err_weight)?,
            due: parse_date(&self.due, DUE_TIME).map_err(|()| s.err_date)?,
            start: parse_date(&self.start, START_TIME).map_err(|()| s.err_date)?,
            notes: (!notes.is_empty()).then(|| notes.to_owned()),
            advance: parse_advance(&self.advance).map_err(|()| s.err_advance)?,
        })
    }
}

impl TaskValues {
    pub fn new_task(self) -> NewTask {
        NewTask {
            name: self.name,
            weight: self.weight,
            due_date: self.due,
            start_date: self.start,
            notes: self.notes,
            prereqs: Vec::new(),
            advance_days: self.advance,
        }
    }

    /// The fields that differ from the ones the form showed at opening.
    pub fn changes(self, opened: &TaskValues) -> Vec<TaskChange> {
        let mut out = Vec::new();
        if self.name != opened.name {
            out.push(TaskChange::Name(self.name));
        }
        if self.weight != opened.weight {
            out.push(TaskChange::Weight(self.weight));
        }
        if self.due != opened.due {
            out.push(TaskChange::DueDate(self.due));
        }
        if self.start != opened.start {
            out.push(TaskChange::StartDate(self.start));
        }
        if self.notes != opened.notes {
            out.push(TaskChange::Notes(self.notes));
        }
        if self.advance != opened.advance {
            out.push(TaskChange::AdvanceDays(self.advance));
        }
        out
    }
}

/// The text of the fields of an area form.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AreaForm {
    pub name: String,
    pub color: String,
    pub weight: String,
    pub advance: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AreaValues {
    pub name: String,
    pub color: String,
    pub weight: Option<f64>,
    pub advance: Option<f64>,
}

impl AreaForm {
    pub fn new(color: &str) -> Self {
        AreaForm { color: color.to_owned(), weight: "10".to_owned(), ..AreaForm::default() }
    }

    pub fn from_area(a: &Area) -> Self {
        AreaForm {
            name: a.name.clone(),
            color: a.color.clone(),
            weight: show_num(a.weight),
            advance: valid_advance_days(a.settings.advance_days).map(|v| v.to_string()).unwrap_or_default(),
        }
    }

    pub fn validate(&self, s: &Strings) -> Result<AreaValues, &'static str> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(s.err_name);
        }
        Ok(AreaValues {
            name: name.to_owned(),
            color: self.color.clone(),
            weight: parse_weight(&self.weight).map_err(|()| s.err_weight)?,
            advance: parse_advance(&self.advance).map_err(|()| s.err_advance)?,
        })
    }
}

impl AreaValues {
    pub fn new_area(self) -> NewArea {
        NewArea { name: self.name, color: Some(self.color), weight: self.weight, advance_days: self.advance }
    }

    pub fn changes(self, opened: &AreaValues) -> Vec<AreaChange> {
        let mut out = Vec::new();
        if self.name != opened.name {
            out.push(AreaChange::Name(self.name));
        }
        if self.color != opened.color {
            out.push(AreaChange::Color(self.color));
        }
        if self.weight != opened.weight {
            out.push(AreaChange::Weight(self.weight));
        }
        if self.advance != opened.advance {
            out.push(AreaChange::AdvanceDays(self.advance));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::IT;

    #[test]
    fn dates() {
        assert_eq!(parse_date("", DUE_TIME), Ok(None));
        assert_eq!(parse_date("2026-12-01", DUE_TIME), Ok(Some("2026-12-01T23:59".to_owned())));
        assert_eq!(parse_date(" 2026-12-01 9:05 ", DUE_TIME), Ok(Some("2026-12-01T09:05".to_owned())));
        assert_eq!(parse_date("2026-02-30", DUE_TIME), Err(()));
        assert_eq!(show_date(Some("2026-12-01T23:59"), DUE_TIME), "2026-12-01");
        assert_eq!(show_date(Some("2026-12-01T09:05"), DUE_TIME), "2026-12-01 09:05");
        assert_eq!(show_date(Some("2026-12-01"), DUE_TIME), "2026-12-01");
    }

    #[test]
    fn task_patch_has_only_changed_fields() {
        let t = TaskNode {
            name: "Call".into(),
            weight: Some(Num(3.0)),
            due_date: Some("2026-12-01T23:59".into()),
            ..TaskNode::default()
        };
        let form = TaskForm::from_task(&t);
        let opened = form.validate(&IT).unwrap_or_else(|_| unreachable!());
        assert!(opened.clone().changes(&opened).is_empty());
        let mut edited = form.clone();
        edited.due.clear();
        edited.notes = "  ".into();
        let changes = edited.validate(&IT).map(|v| v.changes(&opened)).unwrap_or_default();
        assert_eq!(changes, vec![TaskChange::DueDate(None)]);
    }

    #[test]
    fn validation_errors() {
        let mut f = TaskForm::new();
        assert_eq!(f.validate(&IT), Err(IT.err_name));
        f.name = "x".into();
        f.advance = "61".into();
        assert_eq!(f.validate(&IT), Err(IT.err_advance));
        f.advance = String::new();
        f.weight = "0".into();
        assert_eq!(f.validate(&IT), Err(IT.err_weight));
    }
}
