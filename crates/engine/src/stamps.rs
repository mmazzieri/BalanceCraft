//! Typed helpers for the sync metadata (`_meta`) that the store commands write,
//! and the hybrid clock that gives them their timestamps.

use balancecraft_model::{AppData, EntityMeta, Num, TaskNode};

/// Not a tombstone. As in Cerqua, `deletedAt` counts only when it is truthy in
/// JavaScript: 0 and NaN do not delete.
pub fn alive(meta: Option<&EntityMeta>) -> bool {
    meta.and_then(|m| m.deleted_at).is_none_or(|d| d.get() == 0.0 || d.get().is_nan())
}

/// Marks `fields` as updated at `at`, keeping the other stamps.
pub fn touch(meta: &mut Option<EntityMeta>, fields: &[&str], at: f64) {
    let m = meta.get_or_insert_with(EntityMeta::default);
    for f in fields {
        m.fields.insert((*f).to_owned(), Num(at));
    }
}

/// Turns the entity into a tombstone at `at`.
pub fn mark_deleted(meta: &mut Option<EntityMeta>, at: f64) {
    meta.get_or_insert_with(EntityMeta::default).deleted_at = Some(Num(at));
}

/// The parent or the position among siblings changed at `at`.
pub fn touch_structure(meta: &mut Option<EntityMeta>, at: f64) {
    meta.get_or_insert_with(EntityMeta::default).structure = Some(Num(at));
}

fn meta_max(meta: Option<&EntityMeta>) -> f64 {
    let Some(m) = meta else { return 0.0 };
    let mut out = 0.0_f64;
    for v in m.fields.values().map(|n| n.get()).chain(m.structure.map(Num::get)).chain(m.deleted_at.map(Num::get)) {
        if v.is_finite() {
            out = out.max(v);
        }
    }
    out
}

fn task_max(node: &TaskNode) -> f64 {
    node.children.iter().map(task_max).fold(meta_max(node.meta.as_ref()), f64::max)
}

/// Highest timestamp in the document: field stamps, structure stamps and
/// deletions, at any depth.
pub fn max_stamp(data: &AppData) -> f64 {
    let mut out = 0.0_f64;
    for a in &data.areas {
        out = out.max(meta_max(a.meta.as_ref()));
        for c in &a.children {
            out = out.max(task_max(c));
        }
    }
    for h in &data.history {
        out = out.max(meta_max(h.meta.as_ref()));
    }
    for e in data.area_actions.iter().flatten() {
        out = out.max(meta_max(e.meta.as_ref()));
    }
    if let Some(s) = &data.settings {
        out = out.max(meta_max(s.meta.as_ref()));
    }
    out
}

/// Hybrid clock: the stamp of a new edit is `max(now, highest stamp seen + 1)`.
///
/// A PC whose clock runs behind would otherwise stamp an edit older than the
/// value it replaces, and the merge would throw the edit away. The stamps stay
/// milliseconds since the epoch, so Cerqua reads them as its own.
#[allow(clippy::cast_precision_loss)]
pub fn next_stamp(data: &AppData, now_ms: i64) -> f64 {
    (now_ms as f64).max(max_stamp(data) + 1.0)
}
