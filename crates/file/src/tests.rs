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
