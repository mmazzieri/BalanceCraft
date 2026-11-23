//! Field-level last-writer-wins merge of two documents. Port of Cerqua's
//! `mergeAppData` (merge.ts, 1.46.0), on JSON objects like the original, so
//! that every field it does not know travels as it does in Cerqua.
//!
//! The merge is deterministic but not symmetric: `remote` is the arbiter (the
//! row on Supabase for Cerqua, the shared file for BalanceCraft). On equal
//! structure stamps the remote side decides parent and position. The rules
//! are documented in Cerqua's merge.ts and CONTEXT.md (#120, #128, #131, #132).
//!
//! [`Rules::BalanceCraft`] adds two rules to Cerqua's, for format v1 (see
//! docs/plan.md): every field with a stamp in `_meta.fields` on either side
//! takes part in the last-writer-wins comparison, not only the known ones, so
//! an older version does not undo a newer field; and on equal stamps with
//! different values the value with the smaller canonical JSON wins, on both
//! sides, so two copies cannot overwrite each other forever.

use crate::json::{Obj, canonical, children, id, is_deleted, meta, meta_num, num_or_zero, num_value, with_meta};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub(crate) const AREA_FIELDS: &[&str] =
    &["name", "color", "weight", "status", "nextActionText", "nextActionTaskId", "advanceDays"];
pub(crate) const TASK_FIELDS: &[&str] = &["name", "weight", "dueDate", "startDate", "notes", "prereqs", "advanceDays"];
pub(crate) const HISTORY_FIELDS: &[&str] = &["name", "completedAt", "effort", "waiting", "waitingFor", "createdAt"];
pub(crate) const ACTION_FIELDS: &[&str] = &["text", "at"];
pub(crate) const SETTING_KEYS: &[&str] = &["advanceDays"];

/// Which merge rules to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rules {
    /// Exactly Cerqua's merge: the conformance vectors check it.
    Cerqua,
    /// Cerqua's merge plus the format v1 rules on unknown fields and ties.
    BalanceCraft,
}

// ── Insertion-ordered map, like JavaScript's Map ────────────────────────────

struct OMap<V> {
    keys: Vec<String>,
    map: HashMap<String, V>,
}

impl<V> OMap<V> {
    fn new() -> Self {
        OMap { keys: Vec::new(), map: HashMap::new() }
    }
    /// Map.set: a new key goes last, an existing one keeps its place.
    fn set(&mut self, k: &str, v: V) {
        if self.map.insert(k.to_owned(), v).is_none() {
            self.keys.push(k.to_owned());
        }
    }
    fn get(&self, k: &str) -> Option<&V> {
        self.map.get(k)
    }
    fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.keys.iter().filter_map(|k| self.map.get(k).map(|v| (k.as_str(), v)))
    }
}

// ── Entity merge ────────────────────────────────────────────────────────────

fn max_activity(e: &Obj) -> f64 {
    let Some(m) = meta(e) else { return 0.0 };
    let mut max = num_or_zero(m.get("structure"));
    if let Some(Value::Object(fields)) = m.get("fields") {
        for v in fields.values() {
            if let Some(f) = v.as_f64()
                && f > max
            {
                max = f;
            }
        }
    }
    max
}

fn fields(e: &Obj) -> Option<&Obj> {
    meta(e).and_then(|m| m.get("fields")).and_then(Value::as_object)
}

fn field_ts(e: &Obj, field: &str) -> f64 {
    num_or_zero(fields(e).and_then(|f| f.get(field)))
}

/// The fields compared one by one: the known ones and, with the BalanceCraft
/// rules, every other field with a stamp on either side.
fn tracked(local: &Obj, remote: &Obj, known: &[&str], rules: Rules) -> Vec<String> {
    let mut out: Vec<String> = known.iter().map(|s| (*s).to_owned()).collect();
    if rules == Rules::BalanceCraft {
        let mut extra: Vec<String> = [fields(local), fields(remote)]
            .into_iter()
            .flatten()
            .flat_map(|f| f.keys().cloned())
            .filter(|k| !known.contains(&k.as_str()))
            .collect();
        extra.sort();
        extra.dedup();
        out.extend(extra);
    }
    out
}

/// On equal stamps, does the remote value win? Cerqua keeps the local one.
fn remote_wins_tie(local: &Obj, remote: &Obj, field: &str, rules: Rules) -> bool {
    if rules == Rules::Cerqua {
        return false;
    }
    let c = |e: &Obj| e.get(field).map(canonical);
    match (c(local), c(remote)) {
        (Some(l), Some(r)) => r < l,
        (Some(_), None) => true, // absent sorts first
        _ => false,
    }
}

/// Field-level merge of two live entities. `children` is emptied: the tree is
/// rebuilt from the flat map.
fn merge_live(local: &Obj, remote: &Obj, known: &[&str], rules: Rules) -> Obj {
    let mut merged = local.clone();
    let mut next_fields = fields(local).cloned().unwrap_or_default();
    for field in tracked(local, remote, known, rules) {
        let l = field_ts(local, &field);
        let r = field_ts(remote, &field);
        if r > l || (r == l && r > 0.0 && remote_wins_tie(local, remote, &field, rules)) {
            match remote.get(&field) {
                Some(v) => merged.insert(field.clone(), v.clone()),
                None => merged.remove(&field),
            };
            if let Some(v) = fields(remote).and_then(|f| f.get(&field)) {
                next_fields.insert(field, v.clone());
            }
        }
    }
    let structure = meta_num(local, "structure").max(meta_num(remote, "structure"));
    let mut m = Obj::new();
    if !next_fields.is_empty() {
        m.insert("fields".to_owned(), Value::Object(next_fields));
    }
    if structure > 0.0 {
        m.insert("structure".to_owned(), num_value(structure));
    }
    merged.insert("_meta".to_owned(), Value::Object(m));
    if merged.contains_key("children") {
        merged.insert("children".to_owned(), Value::Array(Vec::new()));
    }
    merged
}

/// Merges a pair: the merged entity, or the tombstone when the deletion wins.
fn merge_entity(local: Option<&Obj>, remote: Option<&Obj>, known: &[&str], rules: Rules) -> Option<Obj> {
    let (local, remote) = match (local, remote) {
        (None, None) => return None,
        (None, Some(r)) => return Some(r.clone()),
        (Some(l), None) => return Some(l.clone()),
        (Some(l), Some(r)) => (l, r),
    };
    let l_del = is_deleted(local).then(|| meta_num(local, "deletedAt"));
    let r_del = is_deleted(remote).then(|| meta_num(remote, "deletedAt"));
    Some(match (l_del, r_del) {
        (Some(l), Some(r)) => {
            if l >= r {
                local.clone()
            } else {
                remote.clone()
            }
        }
        (Some(l), None) => {
            if l >= max_activity(remote) {
                local.clone()
            } else {
                merge_live(remote, remote, known, rules)
            }
        }
        (None, Some(r)) => {
            if r >= max_activity(local) {
                remote.clone()
            } else {
                merge_live(local, local, known, rules)
            }
        }
        (None, None) => merge_live(local, remote, known, rules),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Local,
    Remote,
}

impl Side {
    fn other(self) -> Side {
        match self {
            Side::Local => Side::Remote,
            Side::Remote => Side::Local,
        }
    }
}

/// Where the position comes from: a side, or the tail of the parent's children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pos {
    Side(Side),
    Tail,
}

fn position_side(local: Option<&Obj>, remote: Option<&Obj>, merged: &Obj) -> Side {
    let (Some(local), Some(remote)) = (local, remote) else {
        return if remote.is_none() { Side::Local } else { Side::Remote };
    };
    if !is_deleted(merged) && (is_deleted(local) || is_deleted(remote)) {
        return if is_deleted(local) { Side::Remote } else { Side::Local };
    }
    if meta_num(local, "structure") > meta_num(remote, "structure") { Side::Local } else { Side::Remote }
}

// ── Flattened trees ─────────────────────────────────────────────────────────

struct FlatEntity<'a> {
    entity: &'a Obj,
    parent: Option<&'a str>,
    live_index: usize,
}

struct Flat<'a> {
    areas: OMap<FlatEntity<'a>>,
    tasks: OMap<FlatEntity<'a>>,
    siblings: Vec<(Option<&'a str>, Vec<&'a Obj>)>,
}

impl<'a> Flat<'a> {
    fn new(areas: &'a [Value]) -> Self {
        let mut flat = Flat { areas: OMap::new(), tasks: OMap::new(), siblings: Vec::new() };
        let roots: Vec<&Obj> = areas.iter().filter_map(Value::as_object).collect();
        flat.siblings.push((None, roots.clone()));
        let mut live = 0;
        for a in roots {
            flat.areas.set(id(a), FlatEntity { entity: a, parent: None, live_index: live });
            if !is_deleted(a) {
                live += 1;
            }
            flat.walk(a);
        }
        flat
    }

    fn walk(&mut self, parent: &'a Obj) {
        let kids = children(parent);
        let pid = id(parent);
        self.siblings.push((Some(pid), kids.clone()));
        let mut live = 0;
        for c in kids {
            self.tasks.set(id(c), FlatEntity { entity: c, parent: Some(pid), live_index: live });
            if !is_deleted(c) {
                live += 1;
            }
            self.walk(c);
        }
    }

    /// An id that is both an area and a task counts as a task.
    fn entry(&self, id: &str) -> Option<&FlatEntity<'a>> {
        self.tasks.get(id).or_else(|| self.areas.get(id))
    }
}

struct Flats<'a> {
    local: Flat<'a>,
    remote: Flat<'a>,
}

impl<'a> Flats<'a> {
    fn on(&self, side: Side) -> &Flat<'a> {
        match side {
            Side::Local => &self.local,
            Side::Remote => &self.remote,
        }
    }
}

#[derive(Debug, Clone)]
struct Placed {
    entity: Obj,
    parent: Option<String>,
    pos: Pos,
    tail_index: Option<usize>,
}

type PlacedMap = OMap<Placed>;

fn placed_deleted(placed: &PlacedMap, id: &str) -> bool {
    placed.get(id).is_some_and(|p| is_deleted(&p.entity))
}

/// Ancestors from the parent up to the area; stops at the first repeated id.
fn ancestors(id: &str, parent_of: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let mut chain = Vec::new();
    let mut seen: HashSet<String> = HashSet::from([id.to_owned()]);
    let mut next = parent_of(id);
    while let Some(n) = next {
        if !seen.insert(n.clone()) {
            break;
        }
        next = parent_of(&n);
        chain.push(n);
    }
    chain
}

fn ancestors_on(flat: &Flat<'_>, id: &str) -> Vec<String> {
    ancestors(id, |n| flat.entry(n).and_then(|e| e.parent.map(str::to_owned)))
}

fn ancestors_in(placed: &PlacedMap, id: &str) -> Vec<String> {
    ancestors(id, |n| placed.get(n).and_then(|p| p.parent.clone()))
}

fn reachability(placed: &PlacedMap) -> (HashSet<String>, Vec<Vec<String>>) {
    let mut verdict: HashMap<String, bool> = HashMap::new();
    let mut cycles = Vec::new();
    for (start, _) in placed.iter() {
        let mut path: Vec<String> = Vec::new();
        let mut on_path: HashMap<String, usize> = HashMap::new();
        let mut cur: Option<String> = Some(start.to_owned());
        let mut reachable = false;
        loop {
            let Some(i) = cur else {
                reachable = true;
                break;
            };
            if let Some(&known) = verdict.get(&i) {
                reachable = known;
                break;
            }
            if let Some(&at) = on_path.get(&i) {
                cycles.push(path[at..].to_vec());
                break;
            }
            let Some(p) = placed.get(&i) else { break };
            on_path.insert(i.clone(), path.len());
            path.push(i);
            cur = p.parent.clone();
        }
        for n in path {
            verdict.insert(n, reachable);
        }
    }
    let reachable = verdict.into_iter().filter(|(_, ok)| *ok).map(|(id, _)| id).collect();
    (reachable, cycles)
}

fn cycle_loser(cycle: &[String], placed: &PlacedMap, flats: &Flats<'_>) -> Option<String> {
    let mut loser: Option<(&str, f64)> = None;
    for id in cycle {
        let (Some(local), Some(remote)) = (flats.local.entry(id), flats.remote.entry(id)) else { continue };
        if local.parent == remote.parent {
            continue;
        }
        let from = match placed.get(id).map(|p| p.pos) {
            Some(Pos::Side(Side::Local)) => local,
            _ => remote,
        };
        let stamp = meta_num(from.entity, "structure");
        let better = match loser {
            None => true,
            Some((lid, ls)) => stamp < ls || (stamp == ls && id.as_str() < lid),
        };
        if better {
            loser = Some((id, stamp));
        }
    }
    loser.map(|(id, _)| id.to_owned())
}

struct Move {
    id: String,
    parent: String,
    pos: Pos,
    from: Side,
    revive: Option<String>,
}

fn loser_move(id: &str, placed: &PlacedMap, flats: &Flats<'_>, reachable: &HashSet<String>) -> Option<Move> {
    let loser = placed.get(id)?;
    let Pos::Side(own) = loser.pos else { return None };
    let other = own.other();
    let alive = !is_deleted(&loser.entity);
    let fits = |n: &String| reachable.contains(n) && (!alive || !placed_deleted(placed, n));

    let theirs = ancestors_on(flats.on(other), id);
    if let Some(target) = theirs.iter().find(|n| fits(n)) {
        let back = Some(target) == theirs.first() && flats.on(other).entry(id).is_some_and(|e| !is_deleted(e.entity));
        let pos = if back { Pos::Side(other) } else { Pos::Tail };
        return Some(Move { id: id.to_owned(), parent: target.clone(), pos, from: own, revive: None });
    }
    if let Some(mine) = ancestors_on(flats.on(own), id).into_iter().find(|n| fits(n)) {
        return Some(Move { id: id.to_owned(), parent: mine, pos: Pos::Tail, from: own, revive: None });
    }
    let area = theirs.last()?;
    let top = placed.get(area)?;
    let dead_area = top.parent.is_none() && is_deleted(&top.entity);
    (alive && dead_area).then(|| Move {
        id: id.to_owned(),
        parent: area.clone(),
        pos: Pos::Tail,
        from: own,
        revive: Some(area.clone()),
    })
}

fn is_visible(id: &str, placed: &PlacedMap, reachable: &HashSet<String>) -> bool {
    reachable.contains(id)
        && std::iter::once(id.to_owned())
            .chain(ancestors_in(placed, id))
            .all(|n| placed.get(&n).is_some_and(|p| !is_deleted(&p.entity)))
}

fn orphan_move(id: &str, placed: &PlacedMap, flats: &Flats<'_>, reachable: &HashSet<String>) -> Option<Move> {
    let Pos::Side(own) = placed.get(id)?.pos else { return None };
    let other = own.other();
    let visible = |n: &str| is_visible(n, placed, reachable);
    let mv =
        |parent: String, pos: Pos, revive: Option<String>| Move { id: id.to_owned(), parent, pos, from: own, revive };

    if let Some(there) = flats.on(other).entry(id)
        && let Some(pp) = there.parent
        && !is_deleted(there.entity)
        && visible(pp)
    {
        return Some(mv(pp.to_owned(), Pos::Side(other), None));
    }
    let above = ancestors_in(placed, id);
    if let Some(alive) = above.iter().find(|n| !placed_deleted(placed, n)) {
        return Some(mv(alive.clone(), Pos::Tail, None));
    }
    if let Some(theirs) = ancestors_on(flats.on(other), id).into_iter().find(|n| visible(n)) {
        return Some(mv(theirs, Pos::Tail, None));
    }
    let area = above.last()?;
    if placed.get(area)?.parent.is_some() {
        return None;
    }
    let pos = if Some(area) == above.first() { Pos::Side(own) } else { Pos::Tail };
    Some(mv(area.clone(), pos, Some(area.clone())))
}

fn revive(id: &str, placed: &mut PlacedMap, flats: &Flats<'_>, rules: Rules) {
    let local = flats.local.entry(id).map(|e| e.entity);
    let remote = flats.remote.entry(id).map(|e| e.entity);
    let (Some(l), Some(r)) = (local.or(remote), remote.or(local)) else { return };
    let content = merge_live(l, r, AREA_FIELDS, rules);
    let stamp = [local, remote]
        .into_iter()
        .flatten()
        .flat_map(|e| [meta_num(e, "structure"), meta_num(e, "deletedAt")])
        .fold(f64::NEG_INFINITY, f64::max);
    let pos = if local.is_some_and(|e| !is_deleted(e)) {
        Pos::Side(Side::Local)
    } else if remote.is_some_and(|e| !is_deleted(e)) {
        Pos::Side(Side::Remote)
    } else {
        Pos::Tail
    };
    let entity = with_meta(&content, "structure", stamp + 1.0);
    placed.set(id, Placed { entity, parent: None, pos, tail_index: None });
}

fn stamped(p: &Placed, id: &str, flats: &Flats<'_>) -> Obj {
    if is_deleted(&p.entity) {
        return p.entity.clone();
    }
    let on = |f: &Flat<'_>| f.entry(id).map_or(0.0, |e| meta_num(e.entity, "structure"));
    with_meta(&p.entity, "structure", on(&flats.local).max(on(&flats.remote)) + 1.0)
}

fn apply_moves(moves: Vec<Option<Move>>, placed: &mut PlacedMap, flats: &Flats<'_>, rules: Rules) {
    for mv in moves.into_iter().flatten() {
        if let Some(area) = &mv.revive {
            revive(area, placed, flats, rules);
        }
        let Some(p) = placed.get(&mv.id) else { continue };
        let entity =
            if p.parent.as_deref() == Some(mv.parent.as_str()) { p.entity.clone() } else { stamped(p, &mv.id, flats) };
        let tail_index = match mv.pos {
            Pos::Tail => flats.on(mv.from).entry(&mv.id).map(|e| e.live_index),
            Pos::Side(_) => None,
        };
        placed.set(&mv.id, Placed { entity, parent: Some(mv.parent), pos: mv.pos, tail_index });
    }
}

fn relocate(placed: &mut PlacedMap, flats: &Flats<'_>, rules: Rules) {
    let (reachable, cycles) = reachability(placed);
    let moves: Vec<Option<Move>> = cycles
        .iter()
        .map(|c| cycle_loser(c, placed, flats).and_then(|l| loser_move(&l, placed, flats, &reachable)))
        .collect();
    apply_moves(moves, placed, flats, rules);

    let (reachable, _) = reachability(placed);
    let orphans: Vec<String> = placed
        .iter()
        .filter(|(id, p)| {
            reachable.contains(*id)
                && !is_deleted(&p.entity)
                && p.parent.as_deref().is_some_and(|pp| placed_deleted(placed, pp))
        })
        .map(|(id, _)| id.to_owned())
        .collect();
    let moves = orphans.iter().map(|id| orphan_move(id, placed, flats, &reachable)).collect();
    apply_moves(moves, placed, flats, rules);
}

fn projected_indices(flat: &Flat<'_>, placed: &PlacedMap) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for (parent, nodes) in &flat.siblings {
        let mut next = 0;
        for node in nodes {
            let nid = id(node);
            let Some(r) = placed.get(nid) else { continue };
            if is_deleted(node) || is_deleted(&r.entity) || r.parent.as_deref() != *parent || r.pos == Pos::Tail {
                continue;
            }
            out.insert(nid.to_owned(), next);
            next += 1;
        }
    }
    out
}

fn merge_list(local: Option<&Value>, remote: Option<&Value>, known: &[&str], rules: Rules) -> Vec<Value> {
    fn objs(v: Option<&Value>) -> Vec<&Obj> {
        v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_object).collect()).unwrap_or_default()
    }
    let (local, remote) = (objs(local), objs(remote));
    let remote_by_id: HashMap<&str, &Obj> = remote.iter().map(|e| (id(e), *e)).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for e in &local {
        if let Some(m) = merge_entity(Some(e), remote_by_id.get(id(e)).copied(), known, rules) {
            out.push(Value::Object(m));
        }
        seen.insert(id(e));
    }
    for e in remote {
        if !seen.contains(id(e)) {
            out.push(Value::Object(e.clone()));
        }
    }
    out
}

/// Merges two documents in their JSON form (`AppData`). `remote` is the arbiter.
pub fn merge_json(local: &Value, remote: &Value, rules: Rules) -> Value {
    fn areas_of(v: &Value) -> &[Value] {
        v.get("areas").and_then(Value::as_array).map_or(&[], Vec::as_slice)
    }
    let (l_areas, r_areas) = (areas_of(local), areas_of(remote));
    let flats = Flats { local: Flat::new(l_areas), remote: Flat::new(r_areas) };

    // Step 1: content, parent and the side that gives the position.
    let mut placed: PlacedMap = OMap::new();
    let mut place = |id: &str, lf: Option<&FlatEntity<'_>>, rf: Option<&FlatEntity<'_>>, known: &[&str]| {
        let (le, re) = (lf.map(|f| f.entity), rf.map(|f| f.entity));
        let Some(merged) = merge_entity(le, re, known, rules) else { return };
        let side = position_side(le, re, &merged);
        let from = match side {
            Side::Local => lf,
            Side::Remote => rf,
        };
        let parent = from.and_then(|f| f.parent.map(str::to_owned));
        placed.set(id, Placed { entity: merged, parent, pos: Pos::Side(side), tail_index: None });
    };
    let mut ids: OMap<()> = OMap::new();
    for (k, _) in flats.local.areas.iter().chain(flats.remote.areas.iter()) {
        ids.set(k, ());
    }
    for (k, _) in ids.iter() {
        place(k, flats.local.areas.get(k), flats.remote.areas.get(k), AREA_FIELDS);
    }
    let mut ids: OMap<()> = OMap::new();
    for (k, _) in flats.local.tasks.iter().chain(flats.remote.tasks.iter()) {
        ids.set(k, ());
    }
    for (k, _) in ids.iter() {
        place(k, flats.local.tasks.get(k), flats.remote.tasks.get(k), TASK_FIELDS);
    }

    // Step 1b: cycles and live tasks under a tombstone (#132).
    relocate(&mut placed, &flats, rules);

    // Step 2: index among the live siblings, on the side that gives the position.
    let local_idx = projected_indices(&flats.local, &placed);
    let remote_idx = projected_indices(&flats.remote, &placed);

    // Rebuild.
    let mut children_of: HashMap<Option<&str>, Vec<&Placed>> = HashMap::new();
    for (_, p) in placed.iter() {
        children_of.entry(p.parent.as_deref()).or_default().push(p);
    }
    let ordered = |parent: Option<&str>| -> Vec<&Placed> {
        let list = children_of.get(&parent).cloned().unwrap_or_default();
        let mut deleted: Vec<&Placed> = list.iter().copied().filter(|p| is_deleted(&p.entity)).collect();
        deleted.sort_by(|a, b| id(&a.entity).cmp(id(&b.entity)));
        let from = |side: Side, idx: &HashMap<String, usize>| {
            let mut v: Vec<(&Placed, usize)> = list
                .iter()
                .copied()
                .filter(|p| p.pos == Pos::Side(side) && !is_deleted(&p.entity))
                .map(|p| (p, idx.get(id(&p.entity)).copied().unwrap_or(0)))
                .collect();
            v.sort_by_key(|(_, i)| *i);
            v
        };
        let mut live: Vec<&Placed> = from(Side::Remote, &remote_idx).into_iter().map(|(p, _)| p).collect();
        for (p, index) in from(Side::Local, &local_idx) {
            live.insert(index.min(live.len()), p);
        }
        let mut tail: Vec<&Placed> =
            list.iter().copied().filter(|p| p.pos == Pos::Tail && !is_deleted(&p.entity)).collect();
        tail.sort_by(|a, b| {
            a.tail_index.unwrap_or(0).cmp(&b.tail_index.unwrap_or(0)).then_with(|| id(&a.entity).cmp(id(&b.entity)))
        });
        deleted.into_iter().chain(live).chain(tail).collect()
    };
    fn attach<'p>(
        parent: &str,
        ordered: &dyn Fn(Option<&str>) -> Vec<&'p Placed>,
        path: &mut Vec<String>,
    ) -> Vec<Value> {
        if path.iter().any(|p| p == parent) {
            return Vec::new(); // a repeated id would loop forever
        }
        path.push(parent.to_owned());
        let out = ordered(Some(parent))
            .into_iter()
            .map(|p| {
                let mut e = p.entity.clone();
                e.insert("children".to_owned(), Value::Array(attach(id(&p.entity), ordered, path)));
                Value::Object(e)
            })
            .collect();
        path.pop();
        out
    }
    let areas: Vec<Value> = ordered(None)
        .into_iter()
        .map(|p| {
            let mut e = p.entity.clone();
            e.insert("children".to_owned(), Value::Array(attach(id(&p.entity), &ordered, &mut Vec::new())));
            Value::Object(e)
        })
        .collect();

    let history = merge_list(local.get("history"), remote.get("history"), HISTORY_FIELDS, rules);
    let actions = merge_list(local.get("areaActions"), remote.get("areaActions"), ACTION_FIELDS, rules);
    let settings_of = |v: &Value| v.get("settings").and_then(Value::as_object).cloned().unwrap_or_default();
    let settings =
        merge_entity(Some(&settings_of(local)), Some(&settings_of(remote)), SETTING_KEYS, rules).unwrap_or_default();

    let mut out = Obj::new();
    out.insert("areas".to_owned(), Value::Array(areas));
    out.insert("history".to_owned(), Value::Array(history));
    out.insert("areaActions".to_owned(), Value::Array(actions));
    out.insert("settings".to_owned(), Value::Object(settings));
    let saved = num_or_zero(local.get("_savedAt")).max(num_or_zero(remote.get("_savedAt")));
    if saved > 0.0 {
        out.insert("_savedAt".to_owned(), num_value(saved));
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests;

/// Merges two typed documents. `remote` is the arbiter.
pub fn merge(
    local: &balancecraft_model::AppData,
    remote: &balancecraft_model::AppData,
    rules: Rules,
) -> Result<balancecraft_model::AppData, serde_json::Error> {
    let merged = merge_json(&serde_json::to_value(local)?, &serde_json::to_value(remote)?, rules);
    serde_json::from_value(merged)
}
