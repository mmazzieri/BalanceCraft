use crate::confine::{Access, Confinement};
use crate::{Command, Error, Session};
use balancecraft_engine::Clock;
use balancecraft_file::{Now, Store};
use chrono_tz::Europe::Rome;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

const T0: i64 = 1_795_500_000_000;

struct Pc {
    tmp: tempfile::TempDir,
    doc: PathBuf,
    state: PathBuf,
}

impl Pc {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        for d in ["docs", "read", "docs-other", "state"] {
            fs::create_dir(base.join(d)).unwrap();
        }
        let pc = Self { doc: base.join("docs/Life.balancecraft"), state: base.join("state"), tmp };
        pc.session().create(T0).unwrap();
        pc
    }
    fn dir(&self, name: &str) -> PathBuf {
        self.tmp.path().canonicalize().unwrap().join(name)
    }
    fn session(&self) -> Session {
        Session::new(&self.doc, &self.state, "test", None).unwrap()
    }
    fn confined(&self) -> Session {
        Session::new(&self.doc, &self.state, "test", Some(self.roots())).unwrap()
    }
    fn roots(&self) -> Confinement {
        Confinement::new(&[self.dir("read")], &[self.dir("docs")]).unwrap()
    }
    fn run(&self, v: Value) -> Value {
        self.try_run(&self.session(), v).unwrap()
    }
    fn try_run(&self, s: &Session, v: Value) -> Result<Value, Error> {
        s.run(serde_json::from_value(v).unwrap(), &Clock::at(Rome, T0 + 60_000).unwrap())
    }
    fn add(&self, v: Value) -> String {
        self.run(v)["id"].as_str().unwrap().to_owned()
    }
}

#[test]
fn every_change_reaches_the_document() {
    let pc = Pc::new();
    let home = pc.add(json!({"cmd": "add_area", "name": "Home", "color": "#112233", "weight": 10}));
    let work = pc.add(json!({"cmd": "add_area", "name": "Work"}));
    let t1 = pc.add(json!({"cmd": "add", "parent": home, "name": "Paint", "due": "2026-12-01", "notes": "white"}));
    let t2 = pc.add(json!({"cmd": "add", "parent": home, "name": "Clean", "prereqs": [t1]}));

    let today = pc.run(json!({"cmd": "today"}));
    assert_eq!((&today["total"], &today["tasks"][0]["path"]), (&json!(1), &json!(["Home", "Paint"])));
    assert_eq!(today["tasks"][0]["due"], "2026-12-01T23:59");

    pc.run(json!({"cmd": "edit", "id": t1, "name": "Paint the wall", "due": null, "weight": 8}));
    let list = pc.run(json!({"cmd": "list", "area": home}));
    let task = &list["areas"][0]["tasks"][0];
    assert_eq!((&task["name"], task.get("due"), &task["notes"]), (&json!("Paint the wall"), None, &json!("white")));

    pc.run(json!({"cmd": "hold", "id": t1, "reason": "paint"}));
    assert_eq!(pc.run(json!({"cmd": "list"}))["areas"][0]["tasks"][0]["on_hold"], "paint");
    pc.run(json!({"cmd": "resume", "id": t1}));
    pc.run(json!({"cmd": "complete", "id": t1}));
    assert!(pc.try_run(&pc.session(), json!({"cmd": "complete", "id": t1})).is_err(), "only once");
    assert_eq!(pc.run(json!({"cmd": "today"}))["tasks"][0]["id"], json!(t2), "its prerequisite is done");
    assert_eq!(pc.run(json!({"cmd": "list", "all": true}))["areas"][0]["tasks"][0]["completed"], true);
    pc.run(json!({"cmd": "uncomplete", "id": t1}));

    pc.run(json!({"cmd": "move", "id": t2, "parent": work}));
    assert!(pc.try_run(&pc.session(), json!({"cmd": "move", "id": t2, "parent": t2})).is_err());
    pc.run(json!({"cmd": "edit_area", "id": work, "name": "Job", "status": "ok", "next_action": "call"}));
    let list = pc.run(json!({"cmd": "list", "area": work}));
    let area = &list["areas"][0];
    assert_eq!((&area["name"], &area["status"], &area["next_action"]), (&json!("Job"), &json!("ok"), &json!("call")));
    assert_eq!(area["tasks"][0]["id"], json!(t2));
    pc.run(json!({"cmd": "delete", "id": t2}));
    pc.run(json!({"cmd": "delete", "id": home}));
    assert_eq!(pc.run(json!({"cmd": "today"}))["total"], 0);
    assert_eq!(pc.run(json!({"cmd": "edit_area", "id": work, "name": "Job"}))["changed"], false);

    let copy = pc.dir("docs").join("Copy.balancecraft");
    pc.run(json!({"cmd": "export", "file": copy}));
    assert!(pc.try_run(&pc.session(), json!({"cmd": "export", "file": copy})).is_err(), "never over a file");
    let id = |p: &PathBuf| serde_json::from_slice::<Value>(&fs::read(p).unwrap()).unwrap()["documentId"].clone();
    assert_ne!(id(&copy), id(&pc.doc));
}

/// The app has the document open, with an edit not yet written, while the command line changes it.
#[test]
fn the_app_and_the_command_line_lose_nothing() {
    let pc = Pc::new();
    let area = pc.add(json!({"cmd": "add_area", "name": "Home"}));
    let (mut app, _) = Store::open(&pc.doc, &pc.dir("read"), "app", Now(T0 + 70_000)).unwrap();
    let mut data = app.data().clone();
    data.areas[0].children.push(
        serde_json::from_value(json!({"id": "app", "name": "From the app",
        "_meta": {"fields": {"name": T0 + 70_000}, "structure": T0 + 70_000}}))
        .unwrap(),
    );
    app.set_data(data).unwrap();
    pc.add(json!({"cmd": "add", "parent": area, "name": "From the command line"}));
    app.sync(Now(T0 + 80_000)).unwrap();
    let list = pc.run(json!({"cmd": "list"}));
    let mut names: Vec<&str> =
        list["areas"][0]["tasks"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    assert_eq!(names, ["From the app", "From the command line"]);
    assert_eq!(app.data().areas[0].children.len(), 2);
}

#[test]
fn values_the_app_would_refuse_are_refused() {
    let pc = Pc::new();
    let a = pc.add(json!({"cmd": "add_area", "name": "Home"}));
    for v in [
        json!({"cmd": "add_area", "name": " "}),
        json!({"cmd": "add_area", "name": "x", "color": "red"}),
        json!({"cmd": "add_area", "name": "x", "weight": 0}),
        json!({"cmd": "add", "parent": "none", "name": "x"}),
        json!({"cmd": "add", "parent": a, "name": "x", "due": "2026-02-30"}),
        json!({"cmd": "add", "parent": a, "name": "x", "advance_days": 61}),
        json!({"cmd": "add", "parent": a, "name": "x", "prereqs": ["none"]}),
        json!({"cmd": "complete", "id": "none"}),
        json!({"cmd": "delete", "id": "none"}),
        json!({"cmd": "list", "area": "none"}),
    ] {
        assert!(matches!(pc.try_run(&pc.session(), v.clone()), Err(Error::Invalid(_))), "{v}");
    }
    assert!(serde_json::from_value::<Command>(json!({"cmd": "today", "path": "/etc"})).is_err());
}

#[test]
fn a_confined_session_stays_in_its_roots() {
    let pc = Pc::new();
    let one = [pc.dir("read")];
    assert!(Confinement::new(&one, &[]).is_err());
    assert!(Confinement::new(&[], &one).is_err());
    let secret = pc.dir("docs-other").join("secret.balancecraft");
    fs::write(&secret, "MARKER").unwrap();
    let roots = pc.roots();
    for p in [secret.clone(), pc.dir("docs").join("../docs-other/secret.balancecraft"), "../docs-other/x".into()] {
        assert!(roots.check(&p, Access::Read).is_err(), "{}", p.display());
        let export = json!({"cmd": "export", "file": p});
        assert!(matches!(pc.try_run(&pc.confined(), export), Err(Error::Refused(_))), "{}", p.display());
    }
    let export = json!({"cmd": "export", "file": pc.dir("read").join("x.balancecraft")});
    assert!(matches!(pc.try_run(&pc.confined(), export), Err(Error::Refused(_))), "the read root is not written");
    assert!(pc.try_run(&pc.confined(), json!({"cmd": "export", "file": "Copy.balancecraft"})).is_ok());
    assert!(Session::new(&secret, &pc.state, "test", Some(pc.roots())).is_err());
    assert_eq!(fs::read_to_string(&secret).unwrap(), "MARKER");
}

#[cfg(unix)]
#[test]
fn links_do_not_lead_out_of_the_roots() {
    use std::os::unix::fs::symlink;
    let pc = Pc::new();
    let other = pc.dir("docs-other");
    let (docs, secret) = (pc.dir("docs"), other.join("secret.balancecraft"));
    fs::write(&secret, "MARKER").unwrap();
    symlink(&secret, docs.join("link.balancecraft")).unwrap();
    symlink(other.join("new"), docs.join("dangling")).unwrap();
    symlink(&other, docs.join("dir")).unwrap();
    assert!(Session::new(&docs.join("link.balancecraft"), &pc.state, "test", Some(pc.roots())).is_err());
    for p in ["link.balancecraft", "dangling", "dir/x"] {
        let r = pc.try_run(&pc.confined(), json!({"cmd": "export", "file": p}));
        assert!(matches!(r, Err(Error::Refused(_))), "{p}");
    }
    // The folder where a round moves the conflict copies.
    let merged = docs.join(balancecraft_file::store::MERGED_DIR);
    symlink(&other, &merged).unwrap();
    assert!(matches!(pc.try_run(&pc.confined(), json!({"cmd": "today"})), Err(Error::Refused(_))));
    fs::remove_file(&merged).unwrap();
    assert!(pc.try_run(&pc.confined(), json!({"cmd": "today"})).is_ok());
    fs::hard_link(&secret, docs.join("Life (1).balancecraft")).unwrap();
    assert!(matches!(pc.try_run(&pc.confined(), json!({"cmd": "today"})), Err(Error::Refused(_))));
    assert_eq!(fs::read_to_string(&secret).unwrap(), "MARKER");
    assert_eq!(fs::read_dir(&other).unwrap().count(), 1);
}
