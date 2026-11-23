//! Helpers that read JSON values with JavaScript's rules, for the code ported
//! from Cerqua that works on plain objects.

use serde_json::{Map, Value};

pub(crate) type Obj = Map<String, Value>;

/// JavaScript truthiness.
pub(crate) fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// `value ?? 0` for a number.
pub(crate) fn num_or_zero(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(0.0)
}

/// A number as `JSON.parse` would give it back: whole values as integers.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn num_value(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() <= 9_007_199_254_740_992.0 {
        Value::from(v as i64)
    } else {
        serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
    }
}

pub(crate) fn meta(e: &Obj) -> Option<&Obj> {
    e.get("_meta").and_then(Value::as_object)
}

pub(crate) fn meta_num(e: &Obj, key: &str) -> f64 {
    num_or_zero(meta(e).and_then(|m| m.get(key)))
}

/// `_meta.deletedAt` is truthy.
pub(crate) fn is_deleted(e: &Obj) -> bool {
    truthy(meta(e).and_then(|m| m.get("deletedAt")))
}

pub(crate) fn id(e: &Obj) -> &str {
    e.get("id").and_then(Value::as_str).unwrap_or("")
}

/// `children ?? []`, objects only.
pub(crate) fn children(e: &Obj) -> Vec<&Obj> {
    match e.get("children") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_object).collect(),
        _ => Vec::new(),
    }
}

/// `{ ...e, _meta: { ...e._meta, [key]: v } }`.
pub(crate) fn with_meta(e: &Obj, key: &str, v: f64) -> Obj {
    let mut out = e.clone();
    let mut m = meta(e).cloned().unwrap_or_default();
    m.insert(key.to_owned(), num_value(v));
    out.insert("_meta".to_owned(), Value::Object(m));
    out
}

/// JSON with object keys sorted, for comparisons that must not depend on key order.
pub(crate) fn canonical(v: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                Value::Object(keys.into_iter().map(|k| (k.clone(), sort(&m[k]))).collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    sort(v).to_string()
}
