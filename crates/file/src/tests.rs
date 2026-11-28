use super::*;
use serde_json::json;

const GEN: &str = "BalanceCraft test";
const NOW: &str = "2026-11-23T21:30:00Z";

fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

fn sample() -> Value {
    json!({
        "format": "balancecraft",
        "formatVersion": 1,
        "generator": "BalanceCraft 0.1.0",
        "documentId": "6f1c2a52-6a7e-4c1b-9a43-0d1f2b3c4d5e",
        "savedAt": "2026-11-23T21:00:00Z",
        "data": {
            "areas": [{ "id": "a1", "name": "Lavoro", "color": "#4a90d9", "weight": 30, "children": [] }],
            "history": [],
            "areaActions": [],
            "settings": {}
        }
    })
}

#[test]
fn reads_and_writes_a_document_unchanged() {
    let input = sample();
    let (doc, origin) = read(&bytes(&input), GEN, NOW).unwrap();
    assert_eq!(origin, Origin::BalanceCraft);
    let out: Value = serde_json::from_slice(&write(&doc).unwrap()).unwrap();
    assert_eq!(out, input);
}

#[test]
fn keeps_unknown_envelope_fields_and_extensions() {
    let mut input = sample();
    input["extensions"] = json!({ "com.example.tool": { "pinned": ["a1"] } });
    input["checksum"] = json!("abc");
    let (doc, _) = read(&bytes(&input), GEN, NOW).unwrap();
    let out: Value = serde_json::from_slice(&write(&doc).unwrap()).unwrap();
    assert_eq!(out, input);
}

#[test]
fn refuses_a_newer_format_version() {
    let mut input = sample();
    input["formatVersion"] = json!(2);
    assert!(matches!(read(&bytes(&input), GEN, NOW), Err(ReadError::TooNew { found: 2 })));
}

#[test]
fn refuses_other_json() {
    assert!(matches!(read(br#"{"format":"other"}"#, GEN, NOW), Err(ReadError::NotADocument)));
    assert!(matches!(read(br#"[1,2]"#, GEN, NOW), Err(ReadError::NotADocument)));
    assert!(matches!(read(b"not json", GEN, NOW), Err(ReadError::Json(_))));
}

#[test]
fn opens_a_cerqua_backup_with_a_new_envelope() {
    let backup = json!({
        "areas": [{ "id": "a1", "name": "Casa", "color": "#e67e22", "weight": 25, "children": [] }],
        "history": [],
        "_savedAt": 1_763_900_000_000_i64
    });
    let (doc, origin) = read(&bytes(&backup), GEN, NOW).unwrap();
    assert_eq!(origin, Origin::CerquaBackup);
    assert_eq!(doc.format_version, FORMAT_VERSION);
    assert_eq!(doc.saved_at, NOW);
    assert_eq!(doc.data.areas[0].name, "Casa");
    assert!(uuid::Uuid::parse_str(&doc.document_id).is_ok());
}

#[test]
fn writing_is_deterministic() {
    let (doc, _) = read(&bytes(&sample()), GEN, NOW).unwrap();
    let a = write(&doc).unwrap();
    let (again, _) = read(&a, GEN, NOW).unwrap();
    assert_eq!(write(&again).unwrap(), a);
    assert!(a.ends_with(b"}\n"));
}

#[test]
fn new_documents_get_distinct_ids() {
    let a = new_document(GEN, NOW, AppData::default());
    let b = new_document(GEN, NOW, AppData::default());
    assert_ne!(a.document_id, b.document_id);
}

/// Every value of a real document replaced by a value of another kind, and the
/// document cut short: reading, writing and the engine give an answer, never a panic.
#[test]
fn hostile_documents_never_panic() {
    use balancecraft_engine::{Clock, Rules, compute_flat_list, merge};
    let mut doc = sample();
    doc["data"]["areas"][0]["children"] = json!([
        { "id": "t1", "name": "Paint", "weight": 5, "dueDate": "2026-12-01T18:00", "startDate": "2026-11-01T08:00",
          "notes": "white", "prereqs": ["t2"], "advanceDays": 3,
          "children": [{ "id": "t2", "name": "Buy", "children": [], "_meta": { "fields": { "name": 1 } } }] }
    ]);
    doc["data"]["history"] = json!([{ "id": "h1", "taskId": "t2", "areaId": "a1", "name": "Buy",
        "completedAt": "2026-11-20T10:00:00.000Z", "effort": 2, "waiting": true, "waitingFor": "shop" }]);
    doc["data"]["areaActions"] = json!([{ "id": "x1", "areaId": "a1", "text": "go", "at": "2026-11-20T10:00:00.000Z",
        "source": "manual", "taskId": "t1" }]);
    doc["data"]["settings"] = json!({ "advanceDays": 2, "_meta": { "fields": { "advanceDays": 1 } } });

    fn paths(v: &Value, at: &str, out: &mut Vec<String>) {
        out.push(at.to_owned());
        match v {
            Value::Object(m) => m.iter().for_each(|(k, c)| paths(c, &format!("{at}/{k}"), out)),
            Value::Array(a) => a.iter().enumerate().for_each(|(i, c)| paths(c, &format!("{at}/{i}"), out)),
            _ => {}
        }
    }
    let clock = Clock::at(chrono::Utc, 1_795_500_000_000).unwrap();
    let exercise = |bytes: &[u8]| {
        let Ok((d, _)) = read(bytes, GEN, NOW) else { return };
        let _ = write(&d);
        let _ = compute_flat_list(&d.data.areas, &d.data.history, &clock);
        let _ = merge(&d.data, &AppData::default(), Rules::BalanceCraft);
    };
    let junk: Vec<Value> = serde_json::from_str(
        r#"[null, true, -1, 1e308, -1e308, 0.5, 18446744073709551615, -9223372036854775808,
            "", "9999-99-99", "+275760-09-13T00:00:00Z", [], {}, [null]]"#,
    )
    .unwrap();
    let mut all = Vec::new();
    paths(&doc, "", &mut all);
    for p in &all {
        for j in &junk {
            let mut d = doc.clone();
            if let Some(slot) = d.pointer_mut(p) {
                *slot = j.clone();
            }
            exercise(&bytes(&d));
        }
    }
    let good = bytes(&doc);
    for n in 0..good.len() {
        exercise(&good[..n]);
    }
}

#[test]
fn nesting_past_the_limit_is_an_error() {
    let deep = format!("{}{}", "{\"a\":[".repeat(100_000), "]}".repeat(100_000));
    assert!(read(deep.as_bytes(), GEN, NOW).is_err());
    let mut doc = sample();
    let mut node = json!({ "id": "leaf", "name": "leaf", "children": [] });
    for i in 0..200 {
        node = json!({ "id": format!("t{i}"), "name": "n", "children": [node] });
    }
    doc["data"]["areas"][0]["children"] = json!([node]);
    assert!(read(&bytes(&doc), GEN, NOW).is_err());
}

#[test]
fn a_document_that_would_not_open_is_not_written() {
    let (mut doc, _) = read(&bytes(&sample()), GEN, NOW).unwrap();
    let mut deep = json!(1);
    for _ in 0..200 {
        deep = json!([deep]);
    }
    doc.extensions.insert("com.example.deep".to_owned(), deep);
    assert!(write(&doc).is_err());
}

#[test]
fn a_file_too_large_is_refused_before_it_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.balancecraft");
    std::fs::File::create(&path).unwrap().set_len(MAX_FILE_BYTES + 1).unwrap();
    assert!(read_file(&path).is_err());
}
