use super::*;

fn vectors(name: &str) -> Vec<Value> {
    let path = format!("{}/../../fixtures/conformance/{name}.jsonl", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn cleanup_tombstones_matches_cerqua() {
    for v in vectors("cleanupTombstones") {
        let got = cleanup_tombstones(&v["args"][0], v["args"][1].as_f64().unwrap());
        assert_eq!(got, v["result"], "{}", v["test"]);
    }
}

#[test]
fn reorder_siblings_matches_cerqua() {
    for v in vectors("reorderSiblings") {
        let prev = v["args"][0].as_array().unwrap();
        let next: Vec<&str> = v["args"][1].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
        let got = reorder_siblings(prev, &next, v["args"][2].as_f64().unwrap());
        let got = got.map_or_else(|| v["args"][0].clone(), Value::Array);
        assert_eq!(got, v["result"], "{}", v["test"]);
    }
}

#[test]
fn migrate_legacy_data_matches_cerqua() {
    for v in vectors("migrateLegacyData") {
        assert_eq!(migrate_legacy_data(&v["args"][0]), v["result"], "{}", v["test"]);
    }
}
