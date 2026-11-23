use super::*;
use serde_json::json;

fn vectors(name: &str) -> Vec<Value> {
    let path = format!("{}/../../fixtures/conformance/{name}.jsonl", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn without_saved_at(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.remove("_savedAt");
    }
    v
}

#[test]
fn matches_cerqua_on_every_vector() {
    let all = vectors("mergeAppData");
    assert!(all.len() > 500);
    let mut failures = Vec::new();
    for v in &all {
        let got = without_saved_at(merge_json(&v["args"][0], &v["args"][1], Rules::Cerqua));
        if got != v["result"] {
            failures.push(v["test"].as_str().unwrap_or("").to_owned());
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} differ, first: {:?}",
        failures.len(),
        all.len(),
        &failures[..failures.len().min(5)]
    );
}

fn area(id: &str, extra: Value) -> Value {
    let mut a = json!({ "id": id, "name": id, "color": "#000000", "weight": 10, "children": [] });
    a.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    a
}

#[test]
fn balancecraft_rules_merge_unknown_stamped_fields() {
    let local =
        json!({ "areas": [area("a", json!({ "icon": "old", "_meta": { "fields": { "icon": 1 } } }))], "history": [] });
    let remote =
        json!({ "areas": [area("a", json!({ "icon": "new", "_meta": { "fields": { "icon": 2 } } }))], "history": [] });
    let cerqua = merge_json(&local, &remote, Rules::Cerqua);
    assert_eq!(cerqua["areas"][0]["icon"], "old", "Cerqua ignores fields it does not track");
    let bc = merge_json(&local, &remote, Rules::BalanceCraft);
    assert_eq!(bc["areas"][0]["icon"], "new");
    assert_eq!(bc["areas"][0]["_meta"]["fields"]["icon"], 2);
}

#[test]
fn balancecraft_rules_break_ties_the_same_way_on_both_sides() {
    let a =
        json!({ "areas": [area("x", json!({ "name": "Beta", "_meta": { "fields": { "name": 5 } } }))], "history": [] });
    let b =
        json!({ "areas": [area("x", json!({ "name": "Alfa", "_meta": { "fields": { "name": 5 } } }))], "history": [] });
    let ab = merge_json(&a, &b, Rules::BalanceCraft);
    let ba = merge_json(&b, &a, Rules::BalanceCraft);
    assert_eq!(ab["areas"][0]["name"], "Alfa");
    assert_eq!(ba["areas"][0]["name"], "Alfa");
    // Cerqua keeps the local value: the two sides disagree.
    assert_eq!(merge_json(&a, &b, Rules::Cerqua)["areas"][0]["name"], "Beta");
}

/// The document without tombstones: what the user sees.
fn visible(v: &Value) -> Value {
    fn prune(list: &Value) -> Value {
        Value::Array(
            list.as_array()
                .unwrap()
                .iter()
                .filter(|e| !is_deleted(e.as_object().unwrap()))
                .map(|e| {
                    let mut e = e.clone();
                    if e.get("_meta").is_some_and(|m| m.as_object().is_some_and(serde_json::Map::is_empty)) {
                        e.as_object_mut().unwrap().remove("_meta");
                    }
                    if let Some(c) = e.get("children").cloned() {
                        e["children"] = prune(&c);
                    }
                    e
                })
                .collect(),
        )
    }
    json!({ "areas": prune(&v["areas"]), "history": prune(&v["history"]), "areaActions": prune(&v["areaActions"]) })
}

/// Merging again with the same remote does not change what the user sees (an
/// empty `_meta` that the merge adds does not count), and
/// settles completely by the second time. A tombstone sent back where it was can
/// move once more, invisibly, as Cerqua's CONTEXT.md (#132) says.
#[test]
fn merging_again_with_the_same_remote_settles() {
    for v in vectors("mergeAppData") {
        let (l, r) = (&v["args"][0], &v["args"][1]);
        for rules in [Rules::Cerqua, Rules::BalanceCraft] {
            let once = merge_json(l, r, rules);
            let twice = merge_json(&once, r, rules);
            let thrice = merge_json(&twice, r, rules);
            assert_eq!(visible(&twice), visible(&once), "{}", v["test"]);
            assert_eq!(without_saved_at(thrice), without_saved_at(twice), "{}", v["test"]);
        }
    }
}
