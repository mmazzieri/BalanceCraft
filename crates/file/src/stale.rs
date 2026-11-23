//! A replica not merged for longer than the tombstone lifetime may hold
//! entities that were deleted elsewhere and whose tombstones are already gone:
//! merged as they are, they would come back to life. Before the merge, such a
//! replica drops what the shared file does not have and that nobody touched
//! after the last merge.

use serde_json::{Map, Value};
use std::collections::HashSet;

type Obj = Map<String, Value>;

fn activity(e: &Obj) -> f64 {
    let Some(m) = e.get("_meta").and_then(Value::as_object) else { return 0.0 };
    let mut max: f64 = 0.0;
    for k in ["structure", "deletedAt"] {
        if let Some(v) = m.get(k).and_then(Value::as_f64) {
            max = max.max(v);
        }
    }
    if let Some(f) = m.get("fields").and_then(Value::as_object) {
        for v in f.values().filter_map(Value::as_f64) {
            max = max.max(v);
        }
    }
    max
}

fn ids(v: &Value, out: &mut HashSet<String>) {
    match v {
        Value::Array(a) => a.iter().for_each(|x| ids(x, out)),
        Value::Object(o) => {
            if let Some(id) = o.get("id").and_then(Value::as_str) {
                out.insert(id.to_owned());
            }
            if let Some(c) = o.get("children") {
                ids(c, out);
            }
        }
        _ => {}
    }
}

/// `local` without the entities absent from `disk` and untouched since `since`.
pub fn prune(local: &Value, disk: &Value, since: f64) -> Value {
    let mut known = HashSet::new();
    for key in ["areas", "history", "areaActions"] {
        if let Some(v) = disk.get(key) {
            ids(v, &mut known);
        }
    }
    let stale = |e: &Obj| {
        let id = e.get("id").and_then(Value::as_str).unwrap_or("");
        !known.contains(id) && activity(e) <= since
    };
    fn tree(list: &Value, stale: &dyn Fn(&Obj) -> bool) -> Value {
        let Some(a) = list.as_array() else { return list.clone() };
        Value::Array(
            a.iter()
                .filter_map(|v| {
                    let Some(o) = v.as_object() else { return Some(v.clone()) };
                    let mut o = o.clone();
                    let kids = o.get("children").map(|c| tree(c, stale));
                    let empty = kids.as_ref().is_none_or(|k| k.as_array().is_none_or(Vec::is_empty));
                    if let Some(k) = kids {
                        o.insert("children".to_owned(), k);
                    }
                    (!(stale(&o) && empty)).then_some(Value::Object(o))
                })
                .collect(),
        )
    }
    let mut out = local.clone();
    if let Some(o) = out.as_object_mut() {
        for key in ["areas", "history", "areaActions"] {
            if let Some(v) = local.get(key) {
                o.insert(key.to_owned(), tree(v, &stale));
            }
        }
    }
    out
}
