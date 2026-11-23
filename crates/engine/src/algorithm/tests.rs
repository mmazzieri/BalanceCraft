//! Conformance with Cerqua: fixtures/conformance/{computeShares,computeFlatList}.jsonl
//! hold inputs and outputs recorded from Cerqua's own tests (tools/conformance).

use super::*;
use balancecraft_model::{Area, HistoryEntry};
use serde_json::Value;

fn vectors(name: &str) -> Vec<Value> {
    let path = format!("{}/../../fixtures/conformance/{name}.jsonl", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap();
    text.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn inputs(v: &Value) -> (Vec<Area>, Vec<HistoryEntry>, i64, String) {
    let args = &v["args"];
    let areas = serde_json::from_value(args[0].clone()).unwrap();
    let history = serde_json::from_value(args[1].clone()).unwrap();
    let now_ms = args[2]["nowMs"].as_i64().unwrap();
    let now_dt = args[2]["nowDT"].as_str().unwrap().to_owned();
    (areas, history, now_ms, now_dt)
}

fn close(a: f64, b: &Value) -> bool {
    match b.as_f64() {
        Some(b) => (a - b).abs() <= 1e-9 * b.abs().max(1.0),
        None => a.is_nan(), // JSON.stringify writes NaN as null
    }
}

/// Runs `f` with the clock of the vector, in its time zone.
fn with_clock<R>(
    v: &Value,
    now_ms: i64,
    f: impl FnOnce(&dyn Fn(&str) -> Option<i64>, &Clock<chrono_tz::Tz>) -> R,
) -> R {
    let tz: chrono_tz::Tz = match v["tz"].as_str().unwrap() {
        "" => chrono_tz::UTC,
        name => name.parse().unwrap(),
    };
    let clock = Clock::at(tz, now_ms).unwrap();
    f(&|s| clock.parse(s), &clock)
}

#[test]
fn compute_shares_matches_cerqua() {
    let all = vectors("computeShares");
    assert!(!all.is_empty());
    for v in &all {
        let (areas, history, now_ms, now_dt) = inputs(v);
        with_clock(v, now_ms, |_, clock| {
            assert_eq!(clock.now_dt(), now_dt, "{}", v["test"]);
            let got = compute_shares(&areas, &history, clock);
            let want = &v["result"];
            assert!(close(got.total_spent, &want["totalSpent"]), "{}: totalSpent", v["test"]);
            let want_ids = want["byId"].as_object().unwrap();
            assert_eq!(got.by_id.len(), want_ids.len(), "{}", v["test"]);
            for (id, w) in want_ids {
                let g = got.by_id[id];
                assert!(close(g.share, &w["share"]), "{}: share {id}", v["test"]);
                assert!(close(g.local_share, &w["localShare"]), "{}: localShare {id}", v["test"]);
                assert!(close(g.spent, &w["spent"]), "{}: spent {id}", v["test"]);
            }
        });
    }
}

#[test]
fn compute_flat_list_matches_cerqua() {
    let all = vectors("computeFlatList");
    assert!(!all.is_empty());
    for v in &all {
        let (areas, history, now_ms, now_dt) = inputs(v);
        with_clock(v, now_ms, |_, clock| {
            assert_eq!(clock.now_dt(), now_dt, "{}", v["test"]);
            let got = compute_flat_list(&areas, &history, clock);
            let want = v["result"].as_array().unwrap();
            let got_ids: Vec<&str> = got.iter().map(|t| t.task.id.as_str()).collect();
            let want_ids: Vec<&str> = want.iter().map(|t| t["id"].as_str().unwrap()).collect();
            assert_eq!(got_ids, want_ids, "{}", v["test"]);
            for (g, w) in got.iter().zip(want) {
                let t = &v["test"];
                assert!(close(g.score, &w["score"]), "{t}: score {}", g.task.id);
                assert!(close(g.deficit, &w["deficit"]), "{t}: deficit {}", g.task.id);
                assert_eq!(g.deficit_from, w["deficitFrom"].as_str().unwrap(), "{t}");
                assert_eq!(g.area.id, w["areaId"].as_str().unwrap(), "{t}");
                match (g.days_left, &w["daysLeft"]) {
                    (None, Value::Null) => {}
                    (Some(d), w) => assert!(close(d, w), "{t}: daysLeft {}", g.task.id),
                    (None, w) => panic!("{t}: daysLeft None, want {w}"),
                }
            }
        });
    }
}
