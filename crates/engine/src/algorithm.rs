//! Today's list: which task to do now. Port of Cerqua's `algorithm.ts` (1.46.0).
//!
//! - **Shares.** An area gets weight / sum of the live areas' weights. A branch (a
//!   task with at least one live child) gets its parent's share × weight / sum of
//!   the weights of its sibling branches. Leaves have no share: their weight is
//!   the size of the work.
//! - **Effort.** Each completion of the last 30 days counts its `effort` for every
//!   visible ancestor of the task, found in the whole tree, tombstones included;
//!   if the task is gone, for the area of the history entry.
//! - **Score** of a leaf = deficit × 1000 + weight × 10 + urgency, with the
//!   deficit of its direct parent, branch or area:
//!   max(0, share − effort / total effort).

use crate::clock::Clock;
use balancecraft_model::{Area, EntityMeta, HistoryEntry, Num, TaskNode};
use chrono::TimeZone;
use std::collections::{HashMap, HashSet};

const WINDOW_DAYS: i64 = 30;
const DEFAULT_WEIGHT: f64 = 5.0;
const DAY_MS: f64 = 86_400_000.0;

fn alive(meta: Option<&EntityMeta>) -> bool {
    meta.and_then(|m| m.deleted_at).is_none()
}

/// JavaScript's `weight || fallback`: missing, zero and NaN take the fallback.
fn weight_or(w: Option<Num>, fallback: f64) -> f64 {
    match w.map(Num::get) {
        Some(v) if v != 0.0 && !v.is_nan() => v,
        _ => fallback,
    }
}

fn live_children(node: &TaskNode) -> impl Iterator<Item = &TaskNode> {
    node.children.iter().filter(|c| alive(c.meta.as_ref()))
}

/// A branch is a task with at least one live child.
pub fn is_branch(node: &TaskNode) -> bool {
    live_children(node).next().is_some()
}

/// Share of each branch among the children of a node.
pub fn branch_local_shares(children: &[TaskNode]) -> Vec<(&str, f64)> {
    let branches: Vec<&TaskNode> = children.iter().filter(|c| alive(c.meta.as_ref()) && is_branch(c)).collect();
    let total: f64 = branches.iter().map(|b| weight_or(b.weight, DEFAULT_WEIGHT)).sum();
    branches.into_iter().map(|b| (b.id.as_str(), weight_or(b.weight, DEFAULT_WEIGHT) / total)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShareInfo {
    /// Share of the total: product of the shares along the path.
    pub share: f64,
    /// Share among siblings (for an area, equal to `share`).
    pub local_share: f64,
    /// Effort of the last 30 days in the subtree.
    pub spent: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shares {
    /// Visible areas and branches.
    pub by_id: HashMap<String, ShareInfo>,
    /// Sum of the areas' effort, 1 when zero.
    pub total_spent: f64,
}

/// Shares and effort of areas and branches. `areas` and `history` are complete,
/// tombstones included.
pub fn compute_shares<Tz: TimeZone>(areas: &[Area], history: &[HistoryEntry], clock: &Clock<Tz>) -> Shares {
    let live_areas: Vec<&Area> = areas.iter().filter(|a| alive(a.meta.as_ref())).collect();
    let mut by_id: HashMap<String, ShareInfo> = HashMap::new();
    let total_weight = match live_areas.iter().map(|a| weight_or(a.weight, 1.0)).sum::<f64>() {
        t if t != 0.0 && !t.is_nan() => t,
        _ => 1.0,
    };

    fn assign(children: &[TaskNode], parent_share: f64, by_id: &mut HashMap<String, ShareInfo>) {
        let local: HashMap<&str, f64> = branch_local_shares(children).into_iter().collect();
        for c in children {
            let Some(&local_share) = local.get(c.id.as_str()) else { continue };
            let share = parent_share * local_share;
            by_id.insert(c.id.clone(), ShareInfo { share, local_share, spent: 0.0 });
            assign(&c.children, share, by_id);
        }
    }
    for a in &live_areas {
        let share = weight_or(a.weight, 1.0) / total_weight;
        by_id.insert(a.id.clone(), ShareInfo { share, local_share: share, spent: 0.0 });
        assign(&a.children, share, &mut by_id);
    }

    // Position of every task in the whole tree: area and ancestors. With a
    // repeated id the first occurrence wins.
    struct Path<'a> {
        area_id: &'a str,
        chain: Vec<&'a str>,
    }
    fn index<'a>(nodes: &'a [TaskNode], area_id: &'a str, chain: &[&'a str], out: &mut HashMap<&'a str, Path<'a>>) {
        for n in nodes {
            out.entry(n.id.as_str()).or_insert_with(|| Path { area_id, chain: chain.to_vec() });
            let mut next = chain.to_vec();
            next.push(n.id.as_str());
            index(&n.children, area_id, &next, out);
        }
    }
    let mut path_of: HashMap<&str, Path<'_>> = HashMap::new();
    for a in areas {
        index(&a.children, a.id.as_str(), &[], &mut path_of);
    }

    let cutoff = clock.days_before(WINDOW_DAYS);
    for h in history {
        if !alive(h.meta.as_ref()) {
            continue;
        }
        let Some(done) = clock.parse(h.completed_at.as_deref().unwrap_or("")) else { continue };
        if cutoff.is_none_or(|c| done < c) {
            continue;
        }
        let effort = h.effort.map_or(1.0, Num::get);
        let targets: Vec<&str> = match path_of.get(h.task_id.as_str()) {
            Some(p) if by_id.contains_key(p.area_id) => {
                let mut t = vec![p.area_id];
                t.extend(p.chain.iter().copied());
                t.push(h.task_id.as_str());
                t
            }
            _ => vec![h.area_id.as_str()],
        };
        for id in targets {
            if let Some(s) = by_id.get_mut(id) {
                s.spent += effort;
            }
        }
    }

    let total: f64 = live_areas.iter().filter_map(|a| by_id.get(&a.id)).map(|s| s.spent).sum();
    let total_spent = if total != 0.0 && !total.is_nan() { total } else { 1.0 };
    Shares { by_id, total_spent }
}

/// A leaf of Today's list.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatTask<'a> {
    pub task: &'a TaskNode,
    pub area: &'a Area,
    pub score: f64,
    /// Whole days to the deadline, rounded up; `None` without a deadline.
    /// `Some(NaN)` for a deadline that is not a date, as in Cerqua.
    pub days_left: Option<f64>,
    /// max(0, share − effort / total) of the node it comes from.
    pub deficit: f64,
    /// Id of the branch or area that gives the deficit (the direct parent).
    pub deficit_from: &'a str,
}

/// The available leaves, by priority (stable on equal scores).
pub fn compute_flat_list<'a, Tz: TimeZone>(
    areas: &'a [Area],
    history: &[HistoryEntry],
    clock: &Clock<Tz>,
) -> Vec<FlatTask<'a>> {
    let shares = compute_shares(areas, history, clock);
    let live: Vec<&HistoryEntry> = history.iter().filter(|h| alive(h.meta.as_ref())).collect();
    let completed: HashSet<&str> =
        live.iter().filter(|h| !h.waiting.unwrap_or(false)).map(|h| h.task_id.as_str()).collect();
    let waiting: HashSet<&str> =
        live.iter().filter(|h| h.waiting.unwrap_or(false)).map(|h| h.task_id.as_str()).collect();

    struct Ctx<'c, Tz: TimeZone> {
        shares: &'c Shares,
        completed: &'c HashSet<&'c str>,
        waiting: &'c HashSet<&'c str>,
        clock: &'c Clock<Tz>,
    }

    fn deficit_of(shares: &Shares, id: &str) -> f64 {
        shares.by_id.get(id).map_or(f64::NAN, |s| (s.share - s.spent / shares.total_spent).max(0.0))
    }

    fn traverse<'a, Tz: TimeZone>(
        node: &'a TaskNode,
        area: &'a Area,
        parent_id: &'a str,
        ctx: &Ctx<'_, Tz>,
        out: &mut Vec<FlatTask<'a>>,
    ) {
        if !alive(node.meta.as_ref()) {
            return;
        }
        if is_branch(node) {
            for c in live_children(node) {
                traverse(c, area, &node.id, ctx, out);
            }
            return;
        }
        let id = node.id.as_str();
        if ctx.completed.contains(id) || ctx.waiting.contains(id) {
            return;
        }
        if node.start_date.as_deref().is_some_and(|s| s > ctx.clock.now_dt()) {
            return;
        }
        if let Some(pre) = &node.prereqs
            && !pre.is_empty()
            && !pre.iter().all(|p| ctx.completed.contains(p.as_str()))
        {
            return;
        }
        let deficit = deficit_of(ctx.shares, parent_id);
        let mut urgency = 0.0;
        let mut days_left = None;
        if let Some(due) = node.due_date.as_deref().filter(|d| !d.is_empty()) {
            #[allow(clippy::cast_precision_loss)]
            let dl = ctx.clock.parse(due).map_or(f64::NAN, |ms| ((ms - ctx.clock.now_ms()) as f64 / DAY_MS).ceil());
            days_left = Some(dl);
            urgency = if dl <= 0.0 {
                1000.0
            } else if dl <= 2.0 {
                500.0
            } else if dl <= 7.0 {
                200.0
            } else {
                // Math.max(0, NaN) is NaN in JavaScript.
                let v = 100.0 - dl * 2.0;
                if v.is_nan() { v } else { v.max(0.0) }
            };
        }
        let score = deficit * 1000.0 + weight_or(node.weight, DEFAULT_WEIGHT) * 10.0 + urgency;
        out.push(FlatTask { task: node, area, score, days_left, deficit, deficit_from: parent_id });
    }

    let ctx = Ctx { shares: &shares, completed: &completed, waiting: &waiting, clock };
    let mut result = Vec::new();
    for a in areas.iter().filter(|a| alive(a.meta.as_ref())) {
        for c in &a.children {
            traverse(c, a, &a.id, &ctx, &mut result);
        }
    }
    // Descending score, stable; JavaScript treats a NaN comparison as "equal".
    result.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    result
}

#[cfg(test)]
mod tests;
