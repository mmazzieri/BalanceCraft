//! What the reading commands answer.

use balancecraft_engine::stamps::alive;
use balancecraft_engine::{Clock, compute_flat_list, tree};
use balancecraft_model::{AppData, HistoryEntry, TaskNode};
use chrono::TimeZone;
use serde_json::{Map, Value, json};

fn entries<'a>(data: &'a AppData, id: &'a str) -> impl Iterator<Item = &'a HistoryEntry> {
    data.history.iter().filter(move |h| h.task_id == id && alive(h.meta.as_ref()))
}

pub fn completed(data: &AppData, id: &str) -> bool {
    entries(data, id).any(|h| !h.waiting.unwrap_or(false))
}

fn put(out: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(v) = value {
        out.insert(key.to_owned(), v);
    }
}

fn task(data: &AppData, node: &TaskNode, all: bool) -> Option<Value> {
    if !alive(node.meta.as_ref()) {
        return None;
    }
    let done = completed(data, &node.id);
    if done && !all {
        return None;
    }
    let hold = entries(data, &node.id).find(|h| h.waiting.unwrap_or(false));
    let mut out = Map::new();
    out.insert("id".into(), json!(node.id));
    out.insert("name".into(), json!(node.name));
    put(&mut out, "weight", node.weight.map(|w| json!(w)));
    put(&mut out, "due", node.due_date.as_ref().map(|d| json!(d)));
    put(&mut out, "start", node.start_date.as_ref().map(|d| json!(d)));
    put(&mut out, "notes", node.notes.as_ref().map(|n| json!(n)));
    put(&mut out, "prereqs", node.prereqs.as_ref().filter(|p| !p.is_empty()).map(|p| json!(p)));
    put(&mut out, "advance_days", node.settings.advance_days.map(|d| json!(d)));
    put(&mut out, "completed", done.then(|| json!(true)));
    put(&mut out, "on_hold", hold.map(|h| json!(h.waiting_for.clone().unwrap_or_default())));
    let children: Vec<Value> = node.children.iter().filter_map(|c| task(data, c, all)).collect();
    put(&mut out, "tasks", (!children.is_empty()).then(|| json!(children)));
    Some(Value::Object(out))
}

pub fn areas(data: &AppData, only: Option<&str>, all: bool) -> Value {
    let list: Vec<Value> = data
        .areas
        .iter()
        .filter(|a| alive(a.meta.as_ref()) && only.is_none_or(|id| a.id == id))
        .map(|a| {
            let mut out = Map::new();
            out.insert("id".into(), json!(a.id));
            out.insert("name".into(), json!(a.name));
            out.insert("color".into(), json!(a.color));
            put(&mut out, "weight", a.weight.map(|w| json!(w)));
            put(&mut out, "status", a.status.as_ref().map(|s| json!(s)));
            put(&mut out, "next_action", a.next_action_text.as_ref().map(|s| json!(s)));
            let tasks: Vec<Value> = a.children.iter().filter_map(|c| task(data, c, all)).collect();
            out.insert("tasks".into(), json!(tasks));
            Value::Object(out)
        })
        .collect();
    json!({"areas": list})
}

pub fn today<Tz: TimeZone>(data: &AppData, clock: &Clock<Tz>, limit: usize) -> Value {
    let list = compute_flat_list(&data.areas, &data.history, clock);
    let tasks: Vec<Value> = list
        .iter()
        .take(limit)
        .map(|t| {
            let mut out = Map::new();
            out.insert("id".into(), json!(t.task.id));
            out.insert("name".into(), json!(t.task.name));
            let path = tree::path_names(&data.areas, &t.task.id).unwrap_or_default();
            out.insert("path".into(), json!(path));
            out.insert("score".into(), json!(t.score));
            put(&mut out, "due", t.task.due_date.as_ref().map(|d| json!(d)));
            put(&mut out, "days_left", t.days_left.filter(|d| d.is_finite()).map(|d| json!(d)));
            Value::Object(out)
        })
        .collect();
    json!({"total": list.len(), "tasks": tasks})
}
