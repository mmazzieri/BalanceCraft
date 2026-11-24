//! Store commands: every change the interface makes to a document. Port of the
//! actions of Cerqua's `useStore.ts` (1.46.0), with the same stamps, so the
//! merge sees a BalanceCraft edit exactly as it would see a Cerqua one.
//!
//! Commands are pure and serializable: the caller passes the ids of new
//! entities and the time in a [`Ctx`]. A future command line or MCP server can
//! replay them unchanged. [`apply`] returns `None` when a command changes
//! nothing (an unknown or deleted target, a value equal to the stored one):
//! nothing to stamp and nothing to save.

use crate::merge::{AREA_FIELDS, TASK_FIELDS};
use crate::meta::reorder_siblings;
use crate::stamps::{alive, mark_deleted, next_stamp, touch, touch_structure};
use crate::tree::{
    children_of, children_of_mut, extract, find_task, find_visible_area, find_visible_task, is_visible_parent, search,
    update_task,
};
use balancecraft_model::{
    ActionSource, AppData, Area, AreaAction, EntityMeta, GlobalSettings, HistoryEntry, Num, SettingValues, TaskNode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Colour of a new area when none is given, as in Cerqua.
pub const DEFAULT_AREA_COLOR: &str = "#3D6B9C";

/// Weight of a completed task without a weight: the `effort` of its history entry.
const DEFAULT_EFFORT: f64 = 5.0;

/// Time of a command.
#[derive(Debug, Clone, PartialEq)]
pub struct Ctx {
    /// Timestamp for `_meta`, from the hybrid clock ([`next_stamp`]).
    pub stamp: f64,
    /// The same instant as an ISO date-time in UTC, as JavaScript's
    /// `toISOString`: for `completedAt`, `createdAt` and `at`.
    pub now_iso: String,
}

impl Ctx {
    /// Time of a command on `data` at `now_ms` (milliseconds since the epoch).
    pub fn new(data: &AppData, now_ms: i64) -> Self {
        let now_iso = DateTime::<Utc>::from_timestamp_millis(now_ms)
            .unwrap_or_default()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        Ctx { stamp: next_stamp(data, now_ms), now_iso }
    }
}

/// Fields of a new area.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewArea {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advance_days: Option<f64>,
}

/// Fields of a new task.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prereqs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advance_days: Option<f64>,
}

/// One edited field of an area. `None` removes the value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "camelCase")]
pub enum AreaChange {
    Name(String),
    Color(String),
    Weight(Option<f64>),
    AdvanceDays(Option<f64>),
}

/// One edited field of a task. `None` removes the value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "camelCase")]
pub enum TaskChange {
    Name(String),
    Weight(Option<f64>),
    DueDate(Option<String>),
    StartDate(Option<String>),
    Notes(Option<String>),
    Prereqs(Vec<String>),
    AdvanceDays(Option<f64>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Command {
    AddArea {
        id: String,
        area: NewArea,
    },
    /// Writes only the fields that differ from the stored ones.
    EditArea {
        id: String,
        changes: Vec<AreaChange>,
    },
    /// Tombstones the area, its tasks and its log entries.
    DeleteArea {
        id: String,
    },
    /// New order of the visible areas; only the order of the ids counts.
    ReorderAreas {
        order: Vec<String>,
    },
    /// A task at the end of a visible area or task.
    AddTask {
        parent_id: String,
        id: String,
        task: NewTask,
    },
    EditTask {
        id: String,
        changes: Vec<TaskChange>,
    },
    /// Tombstones the task and its subtree.
    DeleteTask {
        id: String,
    },
    /// Moves the task under a visible area or task, before the sibling
    /// `before` or, without it, at the end. In the parent it already has, it
    /// changes place in the same list.
    MoveTask {
        id: String,
        new_parent_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
    },
    ReorderChildren {
        parent_id: String,
        order: Vec<String>,
    },
    CompleteTask {
        id: String,
        entry_id: String,
        action_id: String,
    },
    Uncomplete {
        id: String,
    },
    PutOnHold {
        id: String,
        entry_id: String,
        reason: String,
    },
    Resume {
        id: String,
    },
    /// Global warning before deadlines, in working days; `None` restores the default.
    SetAdvanceDays {
        value: Option<f64>,
    },
    /// One-line status of an area; blank removes it.
    SetAreaStatus {
        area_id: String,
        status: String,
    },
    /// Next action of an area, optionally linked to one of its tasks; blank
    /// text removes both.
    SetAreaNextAction {
        area_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
    },
    /// Writes the next action (or `text`, when given) in the area log and clears it.
    CompleteNextAction {
        area_id: String,
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    /// A log entry written by hand.
    LogAreaAction {
        area_id: String,
        id: String,
        text: String,
    },
    /// Changes the text and/or the date of a log entry; blank text and an
    /// invalid date are ignored.
    EditAreaAction {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<String>,
    },
    DeleteAreaAction {
        id: String,
    },
}

/// Applies `cmd` to `data`. `None`: nothing changes.
pub fn apply(data: &AppData, cmd: &Command, ctx: &Ctx) -> Option<AppData> {
    let mut out = data.clone();
    let t = ctx.stamp;
    match cmd {
        Command::AddArea { id, area } => add_area(&mut out, id, area, t),
        Command::EditArea { id, changes } => edit_area(&mut out, id, changes, t)?,
        Command::DeleteArea { id } => delete_area(&mut out, id, t)?,
        Command::ReorderAreas { order } => {
            out.areas = reorder(&out.areas, order, t)?;
        }
        Command::AddTask { parent_id, id, task } => add_task(&mut out, parent_id, id, task, t)?,
        Command::EditTask { id, changes } => edit_task(&mut out, id, changes, t)?,
        Command::DeleteTask { id } => {
            let found = out.areas.iter_mut().any(|a| update_task(&mut a.children, id, &mut |n| tombstone(n, t)));
            if !found {
                return None;
            }
        }
        Command::MoveTask { id, new_parent_id, before } => {
            move_task(&mut out, id, new_parent_id, before.as_deref(), t)?
        }
        Command::ReorderChildren { parent_id, order } => reorder_children(&mut out, parent_id, order, t)?,
        Command::CompleteTask { id, entry_id, action_id } => complete(&mut out, id, entry_id, action_id, ctx)?,
        Command::Uncomplete { id } => uncomplete(&mut out, id, t),
        Command::PutOnHold { id, entry_id, reason } => put_on_hold(&mut out, id, entry_id, reason, ctx)?,
        Command::Resume { id } => {
            for e in out.history.iter_mut().filter(|e| e.task_id == *id && e.waiting.unwrap_or(false)) {
                mark_deleted(&mut e.meta, t);
            }
        }
        Command::SetAdvanceDays { value } => set_advance_days(&mut out, *value, t)?,
        Command::SetAreaStatus { area_id, status } => set_area_status(&mut out, area_id, status, t)?,
        Command::SetAreaNextAction { area_id, text, task_id } => {
            set_next_action(&mut out, area_id, text, task_id.as_deref(), t)?;
        }
        Command::CompleteNextAction { area_id, id, text } => {
            complete_next_action(&mut out, area_id, id, text.as_deref(), ctx)?;
        }
        Command::LogAreaAction { area_id, id, text } => log_action(&mut out, area_id, id, text, ctx)?,
        Command::EditAreaAction { id, text, at } => edit_action(&mut out, id, text.as_deref(), at.as_deref(), t)?,
        Command::DeleteAreaAction { id } => {
            let e = out.area_actions.iter_mut().flatten().find(|e| e.id == *id && alive(e.meta.as_ref()))?;
            mark_deleted(&mut e.meta, t);
        }
    }
    (out != *data).then_some(out)
}

fn num(v: Option<f64>) -> Option<Num> {
    v.map(Num)
}

fn add_area(data: &mut AppData, id: &str, area: &NewArea, t: f64) {
    let mut a = Area {
        id: id.to_owned(),
        name: area.name.clone(),
        color: area.color.clone().unwrap_or_else(|| DEFAULT_AREA_COLOR.to_owned()),
        weight: num(area.weight),
        settings: SettingValues { advance_days: num(area.advance_days) },
        ..Area::default()
    };
    touch(&mut a.meta, AREA_FIELDS, t);
    data.areas.push(a);
}

fn edit_area(data: &mut AppData, id: &str, changes: &[AreaChange], t: f64) -> Option<()> {
    find_visible_area(&data.areas, id)?;
    let a = data.areas.iter_mut().find(|a| a.id == id)?;
    let mut changed: Vec<&str> = Vec::new();
    for c in changes {
        match c {
            AreaChange::Name(v) if *v != a.name => {
                a.name.clone_from(v);
                changed.push("name");
            }
            AreaChange::Color(v) if *v != a.color => {
                a.color.clone_from(v);
                changed.push("color");
            }
            AreaChange::Weight(v) if num(*v) != a.weight => {
                a.weight = num(*v);
                changed.push("weight");
            }
            AreaChange::AdvanceDays(v) if num(*v) != a.settings.advance_days => {
                a.settings.advance_days = num(*v);
                changed.push("advanceDays");
            }
            _ => {}
        }
    }
    if changed.is_empty() {
        return None;
    }
    touch(&mut a.meta, &changed, t);
    Some(())
}

fn tombstone(n: &mut TaskNode, t: f64) {
    for c in &mut n.children {
        tombstone(c, t);
    }
    mark_deleted(&mut n.meta, t);
}

fn delete_area(data: &mut AppData, id: &str, t: f64) -> Option<()> {
    let a = data.areas.iter_mut().find(|a| a.id == id)?;
    for c in &mut a.children {
        tombstone(c, t);
    }
    mark_deleted(&mut a.meta, t);
    for e in data.area_actions.iter_mut().flatten() {
        if e.area_id == id && alive(e.meta.as_ref()) {
            mark_deleted(&mut e.meta, t);
        }
    }
    Some(())
}

/// `reorder_siblings` on typed lists.
fn reorder<T: Serialize + serde::de::DeserializeOwned>(prev: &[T], order: &[String], t: f64) -> Option<Vec<T>> {
    let prev_json: Vec<Value> = prev.iter().filter_map(|v| serde_json::to_value(v).ok()).collect();
    if prev_json.len() != prev.len() {
        return None;
    }
    let ids: Vec<&str> = order.iter().map(String::as_str).collect();
    let next = reorder_siblings(&prev_json, &ids, t)?;
    next.into_iter().map(|v| serde_json::from_value(v).ok()).collect()
}

fn add_task(data: &mut AppData, parent_id: &str, id: &str, task: &NewTask, t: f64) -> Option<()> {
    if !is_visible_parent(&data.areas, parent_id) {
        return None;
    }
    let mut n = TaskNode {
        id: id.to_owned(),
        name: task.name.clone(),
        weight: num(task.weight),
        due_date: task.due_date.clone(),
        start_date: task.start_date.clone(),
        notes: task.notes.clone(),
        prereqs: Some(task.prereqs.clone()),
        settings: SettingValues { advance_days: num(task.advance_days) },
        ..TaskNode::default()
    };
    touch(&mut n.meta, TASK_FIELDS, t);
    children_of_mut(&mut data.areas, parent_id)?.push(n);
    Some(())
}

fn edit_task(data: &mut AppData, id: &str, changes: &[TaskChange], t: f64) -> Option<()> {
    find_visible_task(&data.areas, id)?;
    let mut changed: Vec<&str> = Vec::new();
    let apply_changes = &mut |n: &mut TaskNode| {
        for c in changes {
            match c {
                TaskChange::Name(v) if *v != n.name => {
                    n.name.clone_from(v);
                    changed.push("name");
                }
                TaskChange::Weight(v) if num(*v) != n.weight => {
                    n.weight = num(*v);
                    changed.push("weight");
                }
                TaskChange::DueDate(v) if *v != n.due_date => {
                    n.due_date.clone_from(v);
                    changed.push("dueDate");
                }
                TaskChange::StartDate(v) if *v != n.start_date => {
                    n.start_date.clone_from(v);
                    changed.push("startDate");
                }
                TaskChange::Notes(v) if *v != n.notes => {
                    n.notes.clone_from(v);
                    changed.push("notes");
                }
                TaskChange::Prereqs(v) if v.as_slice() != n.prereqs.as_deref().unwrap_or_default() => {
                    n.prereqs = Some(v.clone());
                    changed.push("prereqs");
                }
                TaskChange::AdvanceDays(v) if num(*v) != n.settings.advance_days => {
                    n.settings.advance_days = num(*v);
                    changed.push("advanceDays");
                }
                _ => {}
            }
        }
        if !changed.is_empty() {
            touch(&mut n.meta, &changed, t);
        }
    };
    // Ids are unique: the first visible match is the one to edit.
    for a in &mut data.areas {
        if update_task(&mut a.children, id, apply_changes) {
            break;
        }
    }
    Some(())
}

/// Order of the live children of `parent_id` with `id` placed before
/// `before` (at the end when `before` is missing or not a live sibling).
fn order_with(data: &AppData, parent_id: &str, id: &str, before: Option<&str>) -> Option<Vec<String>> {
    let mut order: Vec<String> = children_of(&data.areas, parent_id)?
        .iter()
        .filter(|n| n.id != id && alive(n.meta.as_ref()))
        .map(|n| n.id.clone())
        .collect();
    let at = before.and_then(|b| order.iter().position(|x| x == b)).unwrap_or(order.len());
    order.insert(at, id.to_owned());
    Some(order)
}

fn move_task(data: &mut AppData, id: &str, new_parent_id: &str, before: Option<&str>, t: f64) -> Option<()> {
    // Already in that parent: a reorder.
    if children_of(&data.areas, new_parent_id).is_some_and(|c| c.iter().any(|n| n.id == id)) {
        let order = order_with(data, new_parent_id, id, before)?;
        return reorder_children(data, new_parent_id, &order, t);
    }
    // Not into itself, nor under a deleted or missing parent: the subtree
    // would disappear without a tombstone.
    let (moving, _) = find_task(&data.areas, id)?;
    if search(moving, new_parent_id).is_some() || !is_visible_parent(&data.areas, new_parent_id) {
        return None;
    }
    let mut node = extract(&mut data.areas, id)?;
    touch_structure(&mut node.meta, t);
    children_of_mut(&mut data.areas, new_parent_id)?.push(node);
    // Before a sibling: the whole list takes the new order, as a drag in Cerqua.
    if before.is_some() {
        let order = order_with(data, new_parent_id, id, before)?;
        // Already last (unknown `before`): nothing more to stamp.
        let _ = reorder_children(data, new_parent_id, &order, t);
    }
    Some(())
}

fn reorder_children(data: &mut AppData, parent_id: &str, order: &[String], t: f64) -> Option<()> {
    let list = children_of_mut(&mut data.areas, parent_id)?;
    *list = reorder(list, order, t)?;
    Some(())
}

fn complete(data: &mut AppData, id: &str, entry_id: &str, action_id: &str, ctx: &Ctx) -> Option<()> {
    let t = ctx.stamp;
    let (task, area) = find_visible_task(&data.areas, id)?;
    let (name, weight, area_id) = (task.name.clone(), task.weight, area.id.clone());
    let mut entry = HistoryEntry {
        id: entry_id.to_owned(),
        task_id: id.to_owned(),
        area_id: area_id.clone(),
        name: name.clone(),
        completed_at: Some(ctx.now_iso.clone()),
        effort: Some(weight.unwrap_or(Num(DEFAULT_EFFORT))),
        ..HistoryEntry::default()
    };
    touch(&mut entry.meta, &["completedAt", "effort", "name"], t);
    for e in data.history.iter_mut().filter(|e| e.task_id == id && e.waiting.unwrap_or(false)) {
        mark_deleted(&mut e.meta, t);
    }
    data.history.insert(0, entry);
    // Log entry for the area, derived from the task and editable.
    let mut action = AreaAction {
        id: action_id.to_owned(),
        area_id: area_id.clone(),
        text: name,
        at: ctx.now_iso.clone(),
        source: ActionSource::Task,
        task_id: Some(id.to_owned()),
        ..AreaAction::default()
    };
    touch(&mut action.meta, &["text", "at"], t);
    data.area_actions.get_or_insert_with(Vec::new).insert(0, action);
    // The task was the area's next action: it is done.
    if let Some(a) = data.areas.iter_mut().find(|a| a.id == area_id && a.next_action_task_id.as_deref() == Some(id)) {
        a.next_action_text = None;
        a.next_action_task_id = None;
        touch(&mut a.meta, &["nextActionText", "nextActionTaskId"], t);
    }
    Some(())
}

/// A derived log entry the user never edited: its text and date carry the
/// same stamp, the one of the completion.
fn derived_untouched(e: &AreaAction) -> bool {
    if e.source != ActionSource::Task {
        return false;
    }
    let fields = e.meta.as_ref().map(|m| &m.fields);
    match (fields.and_then(|f| f.get("text")), fields.and_then(|f| f.get("at"))) {
        (Some(text), Some(at)) => text == at,
        _ => false,
    }
}

fn uncomplete(data: &mut AppData, id: &str, t: f64) {
    for e in data.history.iter_mut().filter(|e| e.task_id == id && !e.waiting.unwrap_or(false)) {
        mark_deleted(&mut e.meta, t);
    }
    let mut last: Option<AreaAction> = None;
    for e in data.area_actions.iter_mut().flatten() {
        if e.task_id.as_deref() == Some(id) && alive(e.meta.as_ref()) && derived_untouched(e) {
            if last.as_ref().is_none_or(|m| e.at > m.at) {
                last = Some(e.clone());
            }
            mark_deleted(&mut e.meta, t);
        }
    }
    // The completion had closed the linked next action: restore it, unless the
    // user has set one since (completeTask stamps both with the same time).
    let Some(last) = last else { return };
    let clear_stamp = last.meta.as_ref().and_then(|m| m.fields.get("at")).copied();
    for a in &mut data.areas {
        let cleared_by_completion = clear_stamp.is_some()
            && a.meta.as_ref().and_then(|m: &EntityMeta| m.fields.get("nextActionTaskId")).copied() == clear_stamp;
        if a.id == last.area_id
            && a.next_action_text.is_none()
            && a.next_action_task_id.is_none()
            && cleared_by_completion
        {
            a.next_action_text = Some(last.text.clone());
            a.next_action_task_id = Some(id.to_owned());
            touch(&mut a.meta, &["nextActionText", "nextActionTaskId"], t);
        }
    }
}

fn put_on_hold(data: &mut AppData, id: &str, entry_id: &str, reason: &str, ctx: &Ctx) -> Option<()> {
    let t = ctx.stamp;
    let (task, area) = find_visible_task(&data.areas, id)?;
    let mut entry = HistoryEntry {
        id: entry_id.to_owned(),
        task_id: id.to_owned(),
        area_id: area.id.clone(),
        name: task.name.clone(),
        waiting: Some(true),
        waiting_for: Some(reason.to_owned()),
        created_at: Some(ctx.now_iso.clone()),
        ..HistoryEntry::default()
    };
    touch(&mut entry.meta, &["waiting", "waitingFor", "createdAt", "name"], t);
    for e in data.history.iter_mut().filter(|e| e.task_id == id) {
        mark_deleted(&mut e.meta, t);
    }
    data.history.insert(0, entry);
    Some(())
}

fn set_advance_days(data: &mut AppData, value: Option<f64>, t: f64) -> Option<()> {
    let settings = data.settings.get_or_insert_with(GlobalSettings::default);
    if settings.values.advance_days == num(value) {
        return None;
    }
    // A removed override keeps its stamp, so the removal wins the merge.
    settings.values.advance_days = num(value);
    touch(&mut settings.meta, &["advanceDays"], t);
    Some(())
}

fn non_blank(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

fn visible_area_mut<'a>(data: &'a mut AppData, id: &str) -> Option<&'a mut Area> {
    find_visible_area(&data.areas, id)?;
    data.areas.iter_mut().find(|a| a.id == id)
}

fn set_area_status(data: &mut AppData, area_id: &str, status: &str, t: f64) -> Option<()> {
    let a = visible_area_mut(data, area_id)?;
    let value = non_blank(status);
    if a.status == value {
        return None;
    }
    a.status = value;
    touch(&mut a.meta, &["status"], t);
    Some(())
}

fn set_next_action(data: &mut AppData, area_id: &str, text: &str, task_id: Option<&str>, t: f64) -> Option<()> {
    let a = visible_area_mut(data, area_id)?;
    let text = non_blank(text);
    let task_id = text.as_ref().and(task_id).map(str::to_owned);
    if a.next_action_text == text && a.next_action_task_id == task_id {
        return None;
    }
    // Text and linked task are written together, as Cerqua does: taken one by
    // one in the merge, a text could end up linked to another device's task.
    a.next_action_text = text;
    a.next_action_task_id = task_id;
    touch(&mut a.meta, &["nextActionText", "nextActionTaskId"], t);
    Some(())
}

fn push_action(data: &mut AppData, mut action: AreaAction, t: f64) {
    touch(&mut action.meta, &["text", "at"], t);
    data.area_actions.get_or_insert_with(Vec::new).insert(0, action);
}

fn complete_next_action(data: &mut AppData, area_id: &str, id: &str, text: Option<&str>, ctx: &Ctx) -> Option<()> {
    let t = ctx.stamp;
    let a = visible_area_mut(data, area_id)?;
    let text = non_blank(text.or(a.next_action_text.as_deref()).unwrap_or_default())?;
    let task_id = a.next_action_task_id.take();
    a.next_action_text = None;
    touch(&mut a.meta, &["nextActionText", "nextActionTaskId"], t);
    let action = AreaAction {
        id: id.to_owned(),
        area_id: area_id.to_owned(),
        text,
        at: ctx.now_iso.clone(),
        source: ActionSource::Manual,
        task_id,
        ..AreaAction::default()
    };
    push_action(data, action, t);
    Some(())
}

fn log_action(data: &mut AppData, area_id: &str, id: &str, text: &str, ctx: &Ctx) -> Option<()> {
    find_visible_area(&data.areas, area_id)?;
    let action = AreaAction {
        id: id.to_owned(),
        area_id: area_id.to_owned(),
        text: non_blank(text)?,
        at: ctx.now_iso.clone(),
        source: ActionSource::Manual,
        ..AreaAction::default()
    };
    push_action(data, action, ctx.stamp);
    Some(())
}

/// A date-time as JavaScript's `toISOString` writes it, from an ISO date-time
/// with an offset or a bare date (UTC midnight, as `Date.parse` reads it).
pub fn normalize_iso(s: &str) -> Option<String> {
    let s = s.trim();
    let dt = DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)).ok().or_else(|| {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)).map(|d| d.and_utc())
    })?;
    Some(dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
}

fn edit_action(data: &mut AppData, id: &str, text: Option<&str>, at: Option<&str>, t: f64) -> Option<()> {
    let e = data.area_actions.iter_mut().flatten().find(|e| e.id == id && alive(e.meta.as_ref()))?;
    let mut changed: Vec<&str> = Vec::new();
    if let Some(v) = text.and_then(non_blank)
        && v != e.text
    {
        e.text = v;
        changed.push("text");
    }
    // Normalized, because lists sort by string comparison.
    if let Some(v) = at.and_then(normalize_iso)
        && v != e.at
    {
        e.at = v;
        changed.push("at");
    }
    if changed.is_empty() {
        return None;
    }
    touch(&mut e.meta, &changed, t);
    Some(())
}

#[cfg(test)]
mod tests;
