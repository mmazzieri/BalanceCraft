//! Sync metadata operations. Port of Cerqua's `meta.ts` (1.46.0), on JSON
//! objects like the merge: tombstone cleanup, sibling reordering and the
//! migration of data written before `_meta` existed.

use crate::json::{Obj, children, id, is_deleted, meta, meta_num, num_or_zero, num_value, with_meta};
use crate::merge::{ACTION_FIELDS, AREA_FIELDS, HISTORY_FIELDS, SETTING_KEYS, TASK_FIELDS};
use serde_json::Value;
use std::collections::HashMap;

/// Tombstones stay 30 days, for the merge between copies.
pub const TOMBSTONE_TTL_MS: f64 = 30.0 * 24.0 * 60.0 * 60.0 * 1000.0;

fn objs(v: Option<&Value>) -> Vec<&Obj> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_object).collect()).unwrap_or_default()
}

fn array(items: Vec<Obj>) -> Value {
    Value::Array(items.into_iter().map(Value::Object).collect())
}

/// Removes tombstones older than [`TOMBSTONE_TTL_MS`]. A tombstone with a live
/// descendant stays, with its subtree.
pub fn cleanup_tombstones(data: &Value, now_ms: f64) -> Value {
    let cutoff = now_ms - TOMBSTONE_TTL_MS;
    fn has_live_descendant(node: &Obj) -> bool {
        children(node).into_iter().any(|c| !is_deleted(c) || has_live_descendant(c))
    }
    let expired = |e: &Obj| is_deleted(e) && meta_num(e, "deletedAt") < cutoff;
    let purgeable = |n: &Obj| expired(n) && !has_live_descendant(n);
    fn prune(node: &Obj, purgeable: &dyn Fn(&Obj) -> bool) -> Obj {
        let mut out = node.clone();
        let kids: Vec<Obj> =
            children(node).into_iter().filter(|c| !purgeable(c)).map(|c| prune(c, purgeable)).collect();
        out.insert("children".to_owned(), array(kids));
        out
    }
    let mut out = data.as_object().cloned().unwrap_or_default();
    let areas: Vec<Obj> =
        objs(data.get("areas")).into_iter().filter(|a| !purgeable(a)).map(|a| prune(a, &purgeable)).collect();
    let keep =
        |key: &str| -> Value { array(objs(data.get(key)).into_iter().filter(|e| !expired(e)).cloned().collect()) };
    let history = keep("history");
    let actions = keep("areaActions");
    out.insert("areas".to_owned(), array(areas));
    out.insert("history".to_owned(), history);
    out.insert("areaActions".to_owned(), actions);
    Value::Object(out)
}

/// New order of a list of siblings after a drag. `prev` is the stored list,
/// tombstones included; of `next_visible` only the order of the ids counts.
/// Returns `None` when the order does not change (nothing to stamp or save).
///
/// Each live sibling gets a structure stamp: the ones that change place take
/// max(at, s + 1), the others s + 1, with s the stamp they had. Tombstones go
/// last, unstamped.
pub fn reorder_siblings(prev: &[Value], next_visible: &[&str], at: f64) -> Option<Vec<Value>> {
    let prev_visible: Vec<&Obj> = prev.iter().filter_map(Value::as_object).filter(|n| !is_deleted(n)).collect();
    let mut live_by_id: Vec<(&str, &Obj)> = prev_visible.iter().map(|n| (id(n), *n)).collect();
    let mut next: Vec<&Obj> = Vec::new();
    for v in next_visible {
        if let Some(pos) = live_by_id.iter().position(|(i, _)| i == v) {
            next.push(live_by_id.remove(pos).1);
        }
    }
    next.extend(live_by_id.into_iter().map(|(_, n)| n));
    if next.iter().zip(&prev_visible).all(|(a, b)| id(a) == id(b)) {
        return None;
    }
    let prev_index: HashMap<&str, usize> = prev_visible.iter().enumerate().map(|(i, n)| (id(n), i)).collect();
    let mut out: Vec<Value> = next
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let s = meta_num(n, "structure");
            let stamp = if prev_index.get(id(n)) == Some(&i) { s + 1.0 } else { at.max(s + 1.0) };
            Value::Object(with_meta(n, "structure", stamp))
        })
        .collect();
    out.extend(prev.iter().filter(|v| v.as_object().is_some_and(is_deleted)).cloned());
    Some(out)
}

/// Gives `_meta.fields` to entities written before sync metadata existed, with
/// the document's `_savedAt` as the stamp of every tracked field: edits made
/// after that point win, and they do not lose to any later edit elsewhere.
pub fn migrate_legacy_data(data: &Value) -> Value {
    let baseline = num_or_zero(data.get("_savedAt"));
    let ensure = |e: &Obj, fields: &[&str]| -> Obj {
        if meta(e).and_then(|m| m.get("fields")).is_some_and(|f| !f.is_null()) {
            return e.clone();
        }
        let next: Obj = fields.iter().map(|f| ((*f).to_owned(), num_value(baseline))).collect();
        let mut m = meta(e).cloned().unwrap_or_default();
        m.insert("fields".to_owned(), Value::Object(next));
        let mut out = e.clone();
        out.insert("_meta".to_owned(), Value::Object(m));
        out
    };
    fn tree(e: &Obj, fields: &[&str], ensure: &dyn Fn(&Obj, &[&str]) -> Obj) -> Obj {
        let mut out = ensure(e, fields);
        let kids: Vec<Obj> = children(e).into_iter().map(|c| tree(c, TASK_FIELDS, ensure)).collect();
        out.insert("children".to_owned(), array(kids));
        out
    }
    let mut out = data.as_object().cloned().unwrap_or_default();
    let areas = objs(data.get("areas")).into_iter().map(|a| tree(a, AREA_FIELDS, &ensure)).collect();
    let history = objs(data.get("history")).into_iter().map(|e| ensure(e, HISTORY_FIELDS)).collect();
    let actions = objs(data.get("areaActions")).into_iter().map(|e| ensure(e, ACTION_FIELDS)).collect();
    let raw = data.get("settings").and_then(Value::as_object).cloned().unwrap_or_default();
    let present: Vec<&str> =
        SETTING_KEYS.iter().copied().filter(|k| raw.get(*k).is_some_and(|v| !v.is_null())).collect();
    let settings = if present.is_empty() { raw } else { ensure(&raw, &present) };
    out.insert("areas".to_owned(), array(areas));
    out.insert("history".to_owned(), array(history));
    out.insert("areaActions".to_owned(), array(actions));
    out.insert("settings".to_owned(), Value::Object(settings));
    Value::Object(out)
}

#[cfg(test)]
mod tests;
