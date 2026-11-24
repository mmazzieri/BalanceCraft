use serde_json::{Value, json};
use std::path::Path;

fn ask(request: Value) -> Value {
    let answer: Value = serde_json::from_str(&crate::call(&request.to_string())).unwrap();
    answer.get("ok").cloned().unwrap_or_else(|| panic!("{request}: {answer}"))
}

/// A phone: its mirror and its replica.
fn on(dir: &Path, command: Value) -> Value {
    ask(json!({"op": "run", "mirror": dir.join("m.balancecraft"), "state": dir.join("state"), "command": command}))
}

fn carry(from: &Path, to: &Path) {
    std::fs::copy(from.join("m.balancecraft"), to.join("m.balancecraft")).unwrap();
}

/// Two phones change the same document; the document of one, carried to the other, loses nothing.
#[test]
fn a_mirror_replaced_by_another_version_is_merged() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (a.path(), b.path());
    ask(json!({"op": "new", "mirror": a.join("m.balancecraft"), "state": a.join("state")}));
    let area = on(a, json!({"cmd": "add_area", "name": "Home"}))["id"].clone();
    carry(a, b);
    on(a, json!({"cmd": "add", "parent": area, "name": "From a"}));
    on(b, json!({"cmd": "add", "parent": area, "name": "From b"}));
    // b's document takes the place of a's: a's task is in its replica only.
    carry(b, a);
    let names: Vec<Value> = on(a, json!({"cmd": "list"}))["areas"][0]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].clone())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let text = std::fs::read_to_string(a.join("m.balancecraft")).unwrap();
    assert!(text.contains("From a") && text.contains("From b"), "and the mirror has both");
    assert!(ask(json!({"op": "check", "text": text}))["document"].is_string());
}

/// Junk requests are answered with errors, never a panic, through every way in.
#[test]
fn junk_never_panics() {
    for junk in [
        "",
        "null",
        "{}",
        "{\"op\":\"nothing\"}",
        "{\"op\":\"check\",\"text\":\"{\"}",
        "{\"op\":\"check\",\"text\":\"{\\\"areas\\\":[]}\"}",
        "{\"op\":\"run\",\"mirror\":\"/no/such\",\"state\":\"\",\"command\":{\"cmd\":\"today\"}}",
        "{\"op\":\"new\",\"mirror\":\"x\",\"state\":\"\"}",
    ] {
        let answer: Value = serde_json::from_str(&crate::call(junk)).unwrap();
        assert!(answer.get("error").is_some(), "{junk:?}");
    }
    let request = std::ffi::CString::new("{}").unwrap();
    // SAFETY: a valid string in, and the answer freed once.
    #[allow(unsafe_code)]
    unsafe {
        let answer = crate::c::balancecraft_call(request.as_ptr());
        assert!(std::ffi::CStr::from_ptr(answer).to_str().unwrap().contains("\"error\""));
        crate::c::balancecraft_free(answer);
        assert!(!crate::c::balancecraft_call(std::ptr::null()).is_null());
    }
}
