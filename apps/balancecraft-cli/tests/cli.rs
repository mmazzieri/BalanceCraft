//! The built program, from the outside.
#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn cli(dir: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_balancecraft-cli"))
        .arg("--file")
        .arg(dir.join("docs").join("Life.balancecraft"))
        .arg("--state-dir")
        .arg(dir.join("state"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A program that refuses to start closes its input before reading it.
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    child.wait_with_output().unwrap()
}

fn out(o: &Output) -> String {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout.clone()).unwrap().trim_end().to_owned()
}

fn new_document() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("docs")).unwrap();
    std::fs::create_dir(tmp.path().join("in")).unwrap();
    out(&cli(tmp.path(), &["new"], ""));
    tmp
}

#[test]
fn every_command() {
    let tmp = new_document();
    let d = tmp.path();
    assert!(!cli(d, &["new"], "").status.success(), "never over a document");
    let home = out(&cli(d, &["add-area", "Home", "--color", "#112233"], ""));
    let work = out(&cli(d, &["add-area", "Work"], ""));
    out(&cli(d, &["edit-area", &work, "--status", "ok"], ""));
    let a = out(&cli(d, &["add", &home, "Paint", "--due", "2026-12-01"], ""));
    let b = out(&cli(d, &["add", &home, "Clean", "--prereq", &a], ""));
    out(&cli(d, &["edit", &a, "--name", "Paint the wall", "--clear", "due"], ""));
    out(&cli(d, &["hold", &a, "--reason", "paint"], ""));
    assert!(out(&cli(d, &["list"], "")).contains("[on_hold: paint]"));
    out(&cli(d, &["resume", &a], ""));
    assert!(out(&cli(d, &["today"], "")).contains("Home › Paint the wall"));
    out(&cli(d, &["complete", &a], ""));
    out(&cli(d, &["uncomplete", &a], ""));
    out(&cli(d, &["move", &b, &work], ""));
    out(&cli(d, &["delete", &a], ""));
    assert!(!out(&cli(d, &["list", "--all", "--json"], "")).contains("Paint"));
    out(&cli(d, &["export", d.join("copy.balancecraft").to_str().unwrap()], ""));
    assert!(d.join("copy.balancecraft").exists());
    assert!(!cli(d, &["today", "--no-such-flag"], "").status.success());
}

#[test]
fn mcp_speaks_the_protocol_and_needs_both_roots() {
    let tmp = new_document();
    let d = tmp.path();
    let (docs, read) = (d.join("docs"), d.join("in"));
    let (w, r) = (docs.to_str().unwrap(), read.to_str().unwrap());
    assert!(!cli(d, &["mcp"], "").status.success());
    assert!(!cli(d, &["mcp", "--automation-read-root", r], "").status.success());
    assert!(!cli(d, &["mcp", "--automation-write-root", w], "").status.success());
    assert!(!cli(d, &["mcp", "--automation-read-root", w, "--automation-write-root", r], "").status.success());

    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"add_area","arguments":{"name":"Home"}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"export","arguments":{"file":"/etc/x"}}}"#,
        "",
    ]
    .join("\n");
    let text = out(&cli(d, &["mcp", "--automation-read-root", r, "--automation-write-root", w], &input));
    let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "balancecraft");
    assert_eq!(lines[1]["result"]["tools"].as_array().unwrap().len(), 13);
    assert_ne!(lines[2]["result"]["isError"], true);
    assert_eq!(lines[3]["result"]["isError"], true);
    assert!(out(&cli(d, &["list"], "")).contains("Home"));
}
