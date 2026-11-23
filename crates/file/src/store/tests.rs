use super::*;
use balancecraft_model::{Area, EntityMeta, Num};
use std::collections::BTreeMap;

const GEN: &str = "BalanceCraft test";
const DAY: i64 = 86_400_000;
const T0: i64 = 1_763_900_000_000;

fn area(id: &str, name: &str, stamp: i64) -> Area {
    Area {
        id: id.into(),
        name: name.into(),
        color: "#000000".into(),
        weight: Some(Num(10.0)),
        meta: Some(EntityMeta {
            fields: BTreeMap::from([("name".into(), Num::from(stamp))]),
            structure: Some(Num::from(stamp)),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn data(areas: Vec<Area>) -> AppData {
    AppData { areas, ..Default::default() }
}

fn names(s: &Store) -> Vec<String> {
    let mut n: Vec<String> = s
        .data()
        .areas
        .iter()
        .filter(|a| a.meta.as_ref().and_then(|m| m.deleted_at).is_none())
        .map(|a| a.name.clone())
        .collect();
    n.sort();
    n
}

struct Pc {
    dir: tempfile::TempDir,
}

impl Pc {
    fn new() -> Self {
        Pc { dir: tempfile::tempdir().unwrap() }
    }
    fn path(&self) -> &Path {
        self.dir.path()
    }
}

#[test]
fn create_then_open_on_another_computer() {
    let cloud = tempfile::tempdir().unwrap();
    let file = cloud.path().join("Vita.balancecraft");
    let (a, b) = (Pc::new(), Pc::new());
    let sa = Store::create(&file, a.path(), GEN, data(vec![area("w", "Lavoro", T0)]), Now(T0)).unwrap();
    let (sb, report) = Store::open(&file, b.path(), GEN, Now(T0 + 1000)).unwrap();
    assert_eq!(sb.document_id(), sa.document_id());
    assert_eq!(names(&sb), ["Lavoro"]);
    assert!(report.absorbed.is_empty());
}

#[test]
fn the_same_document_cannot_be_opened_twice_on_one_computer() {
    let cloud = tempfile::tempdir().unwrap();
    let file = cloud.path().join("Vita.balancecraft");
    let a = Pc::new();
    let _s = Store::create(&file, a.path(), GEN, data(vec![]), Now(T0)).unwrap();
    assert!(matches!(Store::open(&file, a.path(), GEN, Now(T0)), Err(StoreError::AlreadyOpen)));
}

#[test]
fn an_edit_survives_the_sync_client_replacing_the_file() {
    let cloud = tempfile::tempdir().unwrap();
    let file = cloud.path().join("Vita.balancecraft");
    let (a, b) = (Pc::new(), Pc::new());
    let mut sa = Store::create(&file, a.path(), GEN, data(vec![area("w", "Lavoro", T0)]), Now(T0)).unwrap();
    let old = fs::read(&file).unwrap();
    let (mut sb, _) = Store::open(&file, b.path(), GEN, Now(T0 + 1)).unwrap();

    // A adds an area and writes the file...
    let mut d = sa.data().clone();
    d.areas.push(area("h", "Casa", T0 + 10));
    sa.set_data(d).unwrap();
    assert!(sa.sync(Now(T0 + 20)).unwrap().wrote);
    // ...then the sync client puts back the old version (B's upload won the race).
    fs::write(&file, &old).unwrap();
    // B renames Lavoro meanwhile.
    let mut d = sb.data().clone();
    d.areas[0] = area("w", "Lavoro e studio", T0 + 30);
    sb.set_data(d).unwrap();
    sb.sync(Now(T0 + 40)).unwrap();

    // A's next round brings its area back; B then sees both edits.
    assert!(sa.disk_changed());
    sa.sync(Now(T0 + 50)).unwrap();
    sb.sync(Now(T0 + 60)).unwrap();
    assert_eq!(names(&sa), ["Casa", "Lavoro e studio"]);
    assert_eq!(names(&sb), ["Casa", "Lavoro e studio"]);
    // Settled: another round writes nothing.
    assert!(!sa.sync(Now(T0 + 70)).unwrap().wrote);
    assert!(!sb.sync(Now(T0 + 80)).unwrap().wrote);
}

#[test]
fn conflict_copies_are_merged_and_moved_aside() {
    let cloud = tempfile::tempdir().unwrap();
    let file = cloud.path().join("Vita.balancecraft");
    let a = Pc::new();
    let mut sa = Store::create(&file, a.path(), GEN, data(vec![area("w", "Lavoro", T0)]), Now(T0)).unwrap();

    // Another computer's version, saved by OneDrive as a conflict copy.
    let mut other: Document = read(&fs::read(&file).unwrap(), GEN, "").unwrap().0;
    other.data.areas.push(area("s", "Salute", T0 + 5));
    fs::write(cloud.path().join("Vita-FISSO.balancecraft"), write(&other).unwrap()).unwrap();
    // A copy made by hand, with the same id: reported, not absorbed.
    fs::write(cloud.path().join("Vita - Copia.balancecraft"), write(&other).unwrap()).unwrap();
    // Another document: ignored.
    let unrelated = crate::new_document(GEN, "", data(vec![area("x", "Altro", T0)]));
    fs::write(cloud.path().join("Vita (1).balancecraft"), write(&unrelated).unwrap()).unwrap();

    let report = sa.sync(Now(T0 + 100)).unwrap();
    assert_eq!(names(&sa), ["Lavoro", "Salute"]);
    assert_eq!(report.absorbed.len(), 1);
    assert!(report.absorbed[0].ends_with(".balancecraft-merged/Vita-FISSO.balancecraft"));
    assert!(!cloud.path().join("Vita-FISSO.balancecraft").exists());
    assert_eq!(report.manual_copies, [cloud.path().join("Vita - Copia.balancecraft")]);
    assert!(cloud.path().join("Vita (1).balancecraft").exists());
}

#[test]
fn a_replica_asleep_for_over_a_month_does_not_revive_purged_entities() {
    let cloud = tempfile::tempdir().unwrap();
    let file = cloud.path().join("Vita.balancecraft");
    let (a, b) = (Pc::new(), Pc::new());
    let mut sa =
        Store::create(&file, a.path(), GEN, data(vec![area("w", "Lavoro", T0), area("o", "Vecchia", T0)]), Now(T0))
            .unwrap();
    let (sb, _) = Store::open(&file, b.path(), GEN, Now(T0 + 1)).unwrap();
    drop(sb); // the laptop goes to sleep with "Vecchia" in its replica

    // A deletes "Vecchia"; 40 days later the tombstone is purged.
    let mut d = sa.data().clone();
    d.areas[1].meta.as_mut().unwrap().deleted_at = Some(Num::from(T0 + 10));
    sa.set_data(d).unwrap();
    sa.sync(Now(T0 + 20)).unwrap();
    sa.sync(Now(T0 + 40 * DAY)).unwrap();
    assert!(sa.data().areas.iter().all(|x| x.id != "o"));

    let (sb, _) = Store::open(&file, b.path(), GEN, Now(T0 + 41 * DAY)).unwrap();
    assert_eq!(names(&sb), ["Lavoro"]);
}

#[test]
fn opens_a_cerqua_backup_as_a_new_document() {
    let tmp = tempfile::tempdir().unwrap();
    let backup = tmp.path().join("cerqua-2026-11-23.json");
    fs::write(&backup, br##"{"areas":[{"id":"a1","name":"Casa","color":"#e67e22","weight":25,"children":[]}],"history":[],"_savedAt":1763900000000}"##).unwrap();
    let file = tmp.path().join("Vita.balancecraft");
    let s = Store::import_cerqua_backup(&backup, &file, Pc::new().path(), GEN, Now(T0)).unwrap();
    assert_eq!(names(&s), ["Casa"]);
    let meta = s.data().areas[0].meta.as_ref().unwrap();
    assert_eq!(meta.fields["name"], Num(1_763_900_000_000.0));
    assert!(file.exists());
}
