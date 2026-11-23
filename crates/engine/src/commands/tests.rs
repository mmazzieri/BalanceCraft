use super::*;
use balancecraft_model::{AppData, Num};

fn ctx(stamp: f64) -> Ctx {
    Ctx { stamp, now_iso: "2026-11-23T10:00:00.000Z".to_owned() }
}

fn run(data: &AppData, cmd: Command, stamp: f64) -> AppData {
    apply(data, &cmd, &ctx(stamp)).unwrap_or_else(|| data.clone())
}

fn base() -> AppData {
    let mut d = AppData::default();
    d = run(
        &d,
        Command::AddArea {
            id: "a".into(),
            area: NewArea { name: "Work".into(), weight: Some(30.0), ..NewArea::default() },
        },
        100.0,
    );
    d = run(
        &d,
        Command::AddTask {
            parent_id: "a".into(),
            id: "b".into(),
            task: NewTask { name: "Branch".into(), ..NewTask::default() },
        },
        101.0,
    );
    d = run(
        &d,
        Command::AddTask {
            parent_id: "b".into(),
            id: "t1".into(),
            task: NewTask { name: "One".into(), weight: Some(3.0), ..NewTask::default() },
        },
        102.0,
    );
    run(
        &d,
        Command::AddTask {
            parent_id: "b".into(),
            id: "t2".into(),
            task: NewTask { name: "Two".into(), ..NewTask::default() },
        },
        103.0,
    )
}

fn stamp(meta: Option<&EntityMeta>, field: &str) -> Option<f64> {
    meta.and_then(|m| m.fields.get(field)).map(|n| n.get())
}

#[test]
fn add_stamps_every_tracked_field() {
    let d = base();
    let a = &d.areas[0];
    assert_eq!(a.color, DEFAULT_AREA_COLOR);
    for f in AREA_FIELDS {
        assert_eq!(stamp(a.meta.as_ref(), f), Some(100.0), "{f}");
    }
    let t = &a.children[0].children[0];
    assert_eq!(t.prereqs.as_deref(), Some(&[][..]));
    for f in TASK_FIELDS {
        assert_eq!(stamp(t.meta.as_ref(), f), Some(102.0), "{f}");
    }
    let json = serde_json::to_value(t).unwrap_or_default();
    assert_eq!(json["weight"], 3);
}

#[test]
fn add_under_missing_or_deleted_parent_is_refused() {
    let d = base();
    let cmd = |p: &str| Command::AddTask { parent_id: p.into(), id: "n".into(), task: NewTask::default() };
    assert!(apply(&d, &cmd("nope"), &ctx(200.0)).is_none());
    let deleted = run(&d, Command::DeleteTask { id: "b".into() }, 200.0);
    assert!(apply(&deleted, &cmd("t1"), &ctx(201.0)).is_none());
}

#[test]
fn edit_writes_only_what_changed() {
    let d = base();
    let same = Command::EditTask {
        id: "t1".into(),
        changes: vec![TaskChange::Name("One".into()), TaskChange::Weight(Some(3.0))],
    };
    assert!(apply(&d, &same, &ctx(200.0)).is_none());
    let cmd = Command::EditTask {
        id: "t1".into(),
        changes: vec![TaskChange::Name("One".into()), TaskChange::DueDate(Some("2026-12-01T23:59".into()))],
    };
    let e = run(&d, cmd, 200.0);
    let t = &e.areas[0].children[0].children[0];
    assert_eq!(t.due_date.as_deref(), Some("2026-12-01T23:59"));
    assert_eq!(stamp(t.meta.as_ref(), "dueDate"), Some(200.0));
    assert_eq!(stamp(t.meta.as_ref(), "name"), Some(102.0));
    let cleared = run(&e, Command::EditTask { id: "t1".into(), changes: vec![TaskChange::DueDate(None)] }, 201.0);
    assert_eq!(cleared.areas[0].children[0].children[0].due_date, None);
    let area = run(&d, Command::EditArea { id: "a".into(), changes: vec![AreaChange::Weight(Some(10.0))] }, 202.0);
    assert_eq!(area.areas[0].weight, Some(Num(10.0)));
    assert_eq!(stamp(area.areas[0].meta.as_ref(), "weight"), Some(202.0));
    assert_eq!(stamp(area.areas[0].meta.as_ref(), "name"), Some(100.0));
}

#[test]
fn delete_area_tombstones_subtree_and_log() {
    let d = base();
    let d = run(&d, Command::CompleteTask { id: "t1".into(), entry_id: "h1".into(), action_id: "l1".into() }, 150.0);
    let d = run(&d, Command::DeleteArea { id: "a".into() }, 200.0);
    let a = &d.areas[0];
    assert!(!alive(a.meta.as_ref()));
    assert!(!alive(a.children[0].meta.as_ref()));
    assert!(!alive(a.children[0].children[1].meta.as_ref()));
    let log = d.area_actions.as_deref().unwrap_or_default();
    assert!(!alive(log[0].meta.as_ref()));
    // History stays: it counts for the balance.
    assert!(alive(d.history[0].meta.as_ref()));
}

#[test]
fn move_guards_and_stamps() {
    let d = base();
    // Into its own subtree: refused.
    assert!(apply(&d, &Command::MoveTask { id: "b".into(), new_parent_id: "t1".into() }, &ctx(200.0)).is_none());
    let m = run(&d, Command::MoveTask { id: "t1".into(), new_parent_id: "a".into() }, 200.0);
    let a = &m.areas[0];
    assert_eq!(a.children.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["b", "t1"]);
    assert_eq!(a.children[1].meta.as_ref().and_then(|x| x.structure), Some(Num(200.0)));
    // Same parent: to the end, as a reorder that stamps the siblings.
    let r = run(&d, Command::MoveTask { id: "t1".into(), new_parent_id: "b".into() }, 300.0);
    let kids = &r.areas[0].children[0].children;
    assert_eq!(kids.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["t2", "t1"]);
    assert_eq!(kids[0].meta.as_ref().and_then(|x| x.structure), Some(Num(300.0)));
}

#[test]
fn reorder_children_and_areas() {
    let d = base();
    let d = run(
        &d,
        Command::AddArea { id: "z".into(), area: NewArea { name: "Home".into(), ..NewArea::default() } },
        110.0,
    );
    let r = run(&d, Command::ReorderAreas { order: vec!["z".into(), "a".into()] }, 200.0);
    assert_eq!(r.areas[0].id, "z");
    assert!(apply(&r, &Command::ReorderAreas { order: vec!["z".into(), "a".into()] }, &ctx(201.0)).is_none());
    let c = run(&d, Command::ReorderChildren { parent_id: "b".into(), order: vec!["t2".into(), "t1".into()] }, 200.0);
    assert_eq!(c.areas[0].children[0].children[0].id, "t2");
}

#[test]
fn complete_and_uncomplete_restore_next_action() {
    let mut d = base();
    d.areas[0].next_action_text = Some("One".into());
    d.areas[0].next_action_task_id = Some("t1".into());
    let d = run(&d, Command::PutOnHold { id: "t1".into(), entry_id: "w".into(), reason: "Mario".into() }, 140.0);
    let c = run(&d, Command::CompleteTask { id: "t1".into(), entry_id: "h1".into(), action_id: "l1".into() }, 150.0);
    assert_eq!(c.history[0].effort, Some(Num(3.0)));
    assert_eq!(c.history[0].completed_at.as_deref(), Some("2026-11-23T10:00:00.000Z"));
    assert!(!alive(c.history[1].meta.as_ref()), "the wait is over");
    assert_eq!(c.areas[0].next_action_task_id, None);
    let log = c.area_actions.as_deref().unwrap_or_default();
    assert_eq!((log[0].source, log[0].text.as_str()), (ActionSource::Task, "One"));

    let u = run(&c, Command::Uncomplete { id: "t1".into() }, 160.0);
    assert!(!alive(u.history[0].meta.as_ref()));
    assert!(!alive(u.area_actions.as_deref().unwrap_or_default()[0].meta.as_ref()));
    assert_eq!(u.areas[0].next_action_task_id.as_deref(), Some("t1"));
    assert_eq!(u.areas[0].next_action_text.as_deref(), Some("One"));
}

#[test]
fn hold_and_resume() {
    let d = base();
    let h = run(&d, Command::PutOnHold { id: "t2".into(), entry_id: "w".into(), reason: "Anna".into() }, 200.0);
    assert_eq!(h.history[0].waiting, Some(true));
    assert_eq!(stamp(h.history[0].meta.as_ref(), "waitingFor"), Some(200.0));
    let r = run(&h, Command::Resume { id: "t2".into() }, 201.0);
    assert!(!alive(r.history[0].meta.as_ref()));
}

#[test]
fn global_setting_keeps_stamp_when_removed() {
    let d = base();
    let s = run(&d, Command::SetAdvanceDays { value: Some(3.0) }, 200.0);
    assert!(apply(&s, &Command::SetAdvanceDays { value: Some(3.0) }, &ctx(201.0)).is_none());
    let r = run(&s, Command::SetAdvanceDays { value: None }, 202.0);
    let g = r.settings.unwrap_or_default();
    assert_eq!(g.values.advance_days, None);
    assert_eq!(stamp(g.meta.as_ref(), "advanceDays"), Some(202.0));
}

#[test]
fn hybrid_clock_never_goes_back() {
    let d = base();
    assert_eq!(Ctx::new(&d, 50).stamp, 104.0);
    assert_eq!(Ctx::new(&d, 5_000).stamp, 5_000.0);
    assert_eq!(Ctx::new(&d, 1_763_892_000_123).now_iso, "2025-11-23T10:00:00.123Z");
}

#[test]
fn commands_round_trip_as_json() {
    let cmd =
        Command::EditTask { id: "x".into(), changes: vec![TaskChange::DueDate(None), TaskChange::Weight(Some(2.0))] };
    let json = serde_json::to_string(&cmd).unwrap_or_default();
    assert_eq!(
        json,
        r#"{"op":"editTask","id":"x","changes":[{"field":"dueDate","value":null},{"field":"weight","value":2.0}]}"#
    );
    let back: Command = serde_json::from_str(&json).unwrap_or(Command::Resume { id: String::new() });
    assert_eq!(back, cmd);
}
