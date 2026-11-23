use super::*;
use serde_json::json;

fn round_trip(v: Value) -> Value {
    let data: AppData = serde_json::from_value(v).unwrap();
    serde_json::to_value(&data).unwrap()
}

#[test]
fn reads_a_minimal_cerqua_backup() {
    let data: AppData = serde_json::from_value(json!({
        "areas": [{ "id": "a1", "name": "Lavoro", "color": "#4a90d9", "weight": 30, "children": null }],
        "history": [],
        "_savedAt": 1_700_000_000_000_i64
    }))
    .unwrap();
    assert_eq!(data.areas[0].children.len(), 0);
    assert_eq!(data.areas[0].weight, Some(Num(30.0)));
    assert!(data.area_actions.is_none());
    assert_eq!(data.saved_at, Some(Num(1_700_000_000_000.0)));
}

#[test]
fn tolerates_missing_children_meta_and_decimal_weights() {
    let data: AppData = serde_json::from_value(json!({
        "areas": [{ "id": "a1", "name": "Casa", "weight": 7.5,
                    "children": [{ "id": "t1", "name": "Tetto" }] }],
        "history": null
    }))
    .unwrap();
    let task = &data.areas[0].children[0];
    assert!(task.children.is_empty());
    assert!(task.meta.is_none());
    assert_eq!(data.areas[0].weight, Some(Num(7.5)));
    assert!(data.history.is_empty());
}

#[test]
fn keeps_unknown_fields_everywhere() {
    let input = json!({
        "areas": [{
            "id": "a1", "name": "Salute", "color": "#2ecc71", "weight": 20, "icon": "🌿",
            "children": [{
                "id": "t1", "name": "Corsa", "weight": 3, "children": [], "energy": "high",
                "_meta": { "fields": { "name": 10, "energy": 12 }, "structure": 11, "origin": "pc1" }
            }],
            "_meta": { "fields": { "name": 5 } }
        }],
        "history": [{ "id": "h1", "taskId": "t1", "areaId": "a1", "name": "Corsa",
                      "completedAt": "2026-11-20T08:00:00.000Z", "effort": 3, "mood": 4 }],
        "areaActions": [{ "id": "x1", "areaId": "a1", "text": "Corsa", "at": "2026-11-20T08:00:00.000Z",
                          "source": "task", "taskId": "t1", "pinned": true }],
        "settings": { "advanceDays": 3, "theme": "dark", "_meta": { "fields": { "advanceDays": 1 } } },
        "future": { "x": 1 }
    });
    assert_eq!(round_trip(input.clone()), input);
}

#[test]
fn writes_whole_numbers_without_decimal_point() {
    let out = serde_json::to_string(&Num(5.0)).unwrap();
    assert_eq!(out, "5");
    assert_eq!(serde_json::to_string(&Num(2.5)).unwrap(), "2.5");
    assert_eq!(serde_json::to_string(&Num(1_763_900_000_123.0)).unwrap(), "1763900000123");
}

#[test]
fn tombstones_round_trip() {
    let input = json!({
        "areas": [{ "id": "a1", "name": "Hobby", "color": "#f1c40f", "weight": 10, "children": [],
                    "_meta": { "fields": { "name": 1 }, "structure": 2, "deletedAt": 3 } }],
        "history": []
    });
    assert_eq!(round_trip(input.clone()), input);
}
