//! A document kept in a folder that OneDrive, Google Drive, Dropbox or
//! Syncthing synchronise: see docs/plan.md, "Salvataggio su cartella
//! sincronizzata".
//!
//! Each computer keeps a **replica** of the document in its own data folder
//! (the "local" side of the merge, like Cerqua's `cerqua_data` cache); the shared
//! file is the "row", the remote side and arbiter. Every edit goes to the
//! replica at once. A sync round reads the shared file, absorbs the conflict
//! copies the sync client made, merges the replica in and writes the result
//! back only if it differs. An edit is never lost when the sync client replaces
//! the file in the middle of a round: it stays in the replica, and the next
//! round brings it back.

use crate::{Document, Origin, ReadError, read, write};
use balancecraft_engine::merge::{Rules, merge_json};
use balancecraft_engine::meta::{TOMBSTONE_TTL_MS, cleanup_tombstones, migrate_legacy_data};
use balancecraft_model::AppData;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Extra margin before a tombstone is purged, for clocks that disagree.
const TOMBSTONE_MARGIN_MS: f64 = 24.0 * 60.0 * 60.0 * 1000.0;
/// Copies of the shared file kept before each overwrite.
const BACKUPS: usize = 10;
/// Folder, next to the shared file, where absorbed conflict copies go.
pub const MERGED_DIR: &str = ".balancecraft-merged";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("file error on {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("{path}: {source}")]
    Read { path: PathBuf, source: ReadError },
    #[error("the document is already open in another BalanceCraft window")]
    AlreadyOpen,
    #[error("{0} is not a BalanceCraft document")]
    NotADocument(PathBuf),
    #[error("cannot encode the document: {0}")]
    Encode(#[from] serde_json::Error),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io { path: path.to_owned(), source }
}

/// "Now", for one operation: milliseconds since the epoch.
#[derive(Debug, Clone, Copy)]
pub struct Now(pub i64);

impl Now {
    #[allow(clippy::cast_precision_loss)]
    fn ms(self) -> f64 {
        self.0 as f64
    }
    fn rfc3339(self) -> String {
        chrono::DateTime::from_timestamp_millis(self.0)
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
            .unwrap_or_default()
    }
}

/// What this computer knows of a document, in its own data folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Replica {
    #[serde(rename = "documentId")]
    document_id: String,
    /// Where the shared file was, the last time.
    #[serde(rename = "sharedPath")]
    shared_path: PathBuf,
    /// Last time a round merged the replica with the shared file (ms).
    #[serde(rename = "lastMergedAt")]
    last_merged_at: f64,
    /// SHA-256 of the shared file as the last round read or wrote it.
    #[serde(rename = "diskHash", default)]
    disk_hash: String,
    /// The document, with tombstones and `_meta`: the local side of the merge.
    document: Document,
}

/// What a sync round did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// The shared file had changes from elsewhere.
    pub merged_from_disk: bool,
    /// The shared file was written.
    pub wrote: bool,
    /// Conflict copies merged and moved to [`MERGED_DIR`].
    pub absorbed: Vec<PathBuf>,
    /// Files with the same `documentId` whose name is not a sync client's
    /// conflict copy: probably copies made by hand. The app asks what to do.
    pub manual_copies: Vec<PathBuf>,
}

/// An open document.
pub struct Store {
    shared: PathBuf,
    replica_dir: PathBuf,
    generator: String,
    replica: Replica,
    _lock: File,
}

fn hash(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn to_value(data: &AppData) -> Result<Value, StoreError> {
    Ok(serde_json::to_value(data)?)
}

/// Writes `bytes` to `path` through a temporary file in the same folder and a
/// rename, so a sync client never sees half a file. Sharing violations while a
/// sync client holds the file are retried.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".~{name}.tmp-{}", std::process::id()));
    {
        let mut f = File::create(&tmp).map_err(io_err(&tmp))?;
        f.write_all(bytes).map_err(io_err(&tmp))?;
        f.sync_all().map_err(io_err(&tmp))?;
    }
    let mut delay = std::time::Duration::from_millis(50);
    for attempt in 0..6 {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < 5 && matches!(e.kind(), io::ErrorKind::PermissionDenied) => {
                std::thread::sleep(delay);
                delay *= 2;
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(StoreError::Io { path: path.to_owned(), source: e });
            }
        }
    }
    Ok(())
}

fn read_doc(path: &Path, generator: &str, now: Now) -> Result<(Document, Origin, Vec<u8>), StoreError> {
    let bytes = crate::read_file(path).map_err(io_err(path))?;
    let (doc, origin) =
        read(&bytes, generator, &now.rfc3339()).map_err(|source| StoreError::Read { path: path.to_owned(), source })?;
    Ok((doc, origin, bytes))
}

/// The data of a document as the merge wants it: `_meta` everywhere (legacy
/// data gets the document's save time as baseline).
fn mergeable(doc: &Document) -> Result<Value, StoreError> {
    let mut v = to_value(&doc.data)?;
    if v.get("_savedAt").is_none()
        && let (Some(o), Ok(t)) = (v.as_object_mut(), chrono::DateTime::parse_from_rfc3339(&doc.saved_at))
    {
        o.insert("_savedAt".to_owned(), Value::from(t.timestamp_millis()));
    }
    let mut out = migrate_legacy_data(&v);
    if let Some(o) = out.as_object_mut() {
        o.remove("_savedAt");
    }
    Ok(out)
}

fn same(a: &Value, b: &Value) -> bool {
    balancecraft_engine::merge::canonical_eq(a, b)
}

impl Store {
    fn replica_path(dir: &Path, document_id: &str) -> PathBuf {
        dir.join(format!("{document_id}.replica.json"))
    }

    fn lock(dir: &Path, document_id: &str) -> Result<File, StoreError> {
        fs::create_dir_all(dir).map_err(io_err(dir))?;
        let path = dir.join(format!("{document_id}.lock"));
        let f = File::options().create(true).truncate(false).write(true).open(&path).map_err(io_err(&path))?;
        match f.try_lock() {
            Ok(()) => Ok(f),
            Err(fs::TryLockError::WouldBlock) => Err(StoreError::AlreadyOpen),
            Err(fs::TryLockError::Error(e)) => Err(StoreError::Io { path, source: e }),
        }
    }

    /// Creates a new document at `shared` with `data`.
    pub fn create(
        shared: &Path,
        replica_dir: &Path,
        generator: &str,
        data: AppData,
        now: Now,
    ) -> Result<Store, StoreError> {
        let doc = crate::new_document(generator, &now.rfc3339(), data);
        let lock = Self::lock(replica_dir, &doc.document_id)?;
        let bytes = write(&doc)?;
        write_atomic(shared, &bytes)?;
        let replica = Replica {
            document_id: doc.document_id.clone(),
            shared_path: shared.to_owned(),
            last_merged_at: now.ms(),
            disk_hash: hash(&bytes),
            document: doc,
        };
        let store = Store {
            shared: shared.to_owned(),
            replica_dir: replica_dir.to_owned(),
            generator: generator.to_owned(),
            replica,
            _lock: lock,
        };
        store.save_replica()?;
        Ok(store)
    }

    /// Opens the document at `shared` and runs a first sync round.
    pub fn open(
        shared: &Path,
        replica_dir: &Path,
        generator: &str,
        now: Now,
    ) -> Result<(Store, SyncReport), StoreError> {
        let (doc, origin, _) = read_doc(shared, generator, now)?;
        if origin != Origin::BalanceCraft {
            return Err(StoreError::NotADocument(shared.to_owned()));
        }
        let lock = Self::lock(replica_dir, &doc.document_id)?;
        let rpath = Self::replica_path(replica_dir, &doc.document_id);
        let replica = match crate::read_file(&rpath) {
            Ok(b) => match serde_json::from_slice::<Replica>(&b) {
                Ok(r) => r,
                Err(_) => Replica {
                    document_id: doc.document_id.clone(),
                    shared_path: shared.to_owned(),
                    last_merged_at: 0.0,
                    disk_hash: String::new(),
                    document: doc.clone(),
                },
            },
            Err(_) => Replica {
                document_id: doc.document_id.clone(),
                shared_path: shared.to_owned(),
                last_merged_at: now.ms(),
                disk_hash: String::new(),
                document: doc.clone(),
            },
        };
        let mut store = Store {
            shared: shared.to_owned(),
            replica_dir: replica_dir.to_owned(),
            generator: generator.to_owned(),
            replica,
            _lock: lock,
        };
        store.replica.shared_path = shared.to_owned();
        let report = store.sync(now)?;
        Ok((store, report))
    }

    /// Makes a new `.balancecraft` document at `shared` from a Cerqua backup.
    pub fn import_cerqua_backup(
        backup: &Path,
        shared: &Path,
        replica_dir: &Path,
        generator: &str,
        now: Now,
    ) -> Result<Store, StoreError> {
        let (doc, _, _) = read_doc(backup, generator, now)?;
        let data: AppData = serde_json::from_value(migrate_legacy_data(&to_value(&doc.data)?))?;
        let mut data = data;
        data.saved_at = None;
        Self::create(shared, replica_dir, generator, data, now)
    }

    pub fn document_id(&self) -> &str {
        &self.replica.document.document_id
    }

    pub fn shared_path(&self) -> &Path {
        &self.shared
    }

    /// The document, tombstones included.
    pub fn data(&self) -> &AppData {
        &self.replica.document.data
    }

    /// Replaces the document after an edit. The replica is written at once; the
    /// shared file at the next [`Store::sync`].
    pub fn set_data(&mut self, data: AppData) -> Result<(), StoreError> {
        self.replica.document.data = data;
        self.save_replica()
    }

    fn save_replica(&self) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec(&self.replica)?;
        // As `write`: never a replica that would not be read back.
        serde_json::from_slice::<Replica>(&bytes)?;
        write_atomic(&Self::replica_path(&self.replica_dir, &self.replica.document_id), &bytes)
    }

    /// Has the shared file changed since the last round? Cheap enough to poll.
    pub fn disk_changed(&self) -> bool {
        crate::read_file(&self.shared).map(|b| hash(&b) != self.replica.disk_hash).unwrap_or(false)
    }

    /// One sync round: absorb conflict copies, merge the replica with the shared
    /// file, write the file if the result differs, remember the result.
    pub fn sync(&mut self, now: Now) -> Result<SyncReport, StoreError> {
        let mut report = SyncReport::default();
        let (disk_doc, _, disk_bytes) = read_doc(&self.shared, &self.generator, now)?;
        let mut disk = mergeable(&disk_doc)?;
        let disk_before = disk.clone();
        report.merged_from_disk = hash(&disk_bytes) != self.replica.disk_hash;

        // Conflict copies merge into the shared file first, as the local side,
        // one at a time, by save time and then by name.
        let (copies, manual) = crate::conflicts::find(&self.shared, self.document_id(), &self.generator);
        report.manual_copies = manual;
        for (_, doc) in &copies {
            disk = merge_json(&mergeable(doc)?, &disk, Rules::BalanceCraft);
        }

        let mut local = mergeable(&self.replica.document)?;
        if self.replica.last_merged_at > 0.0 && now.ms() - self.replica.last_merged_at > TOMBSTONE_TTL_MS {
            local = crate::stale::prune(&local, &disk, self.replica.last_merged_at);
        }
        let merged = merge_json(&local, &disk, Rules::BalanceCraft);
        let mut merged = cleanup_tombstones(&merged, now.ms() - TOMBSTONE_MARGIN_MS);
        if let Some(o) = merged.as_object_mut() {
            o.remove("_savedAt");
        }
        let data: AppData = serde_json::from_value(merged.clone())?;

        let mut doc = disk_doc.clone();
        doc.data = data;
        if !same(&merged, &disk_before) {
            doc.generator = self.generator.clone();
            doc.saved_at = now.rfc3339();
            let bytes = write(&doc)?;
            self.backup(&disk_bytes, now)?;
            write_atomic(&self.shared, &bytes)?;
            self.replica.disk_hash = hash(&bytes);
            report.wrote = true;
        } else {
            self.replica.disk_hash = hash(&disk_bytes);
        }
        self.replica.document = doc;
        self.replica.last_merged_at = now.ms();
        self.save_replica()?;

        if !copies.is_empty() {
            report.absorbed = crate::conflicts::move_aside(&self.shared, copies.into_iter().map(|(p, _)| p))?;
        }
        Ok(report)
    }

    fn backup(&self, bytes: &[u8], now: Now) -> Result<(), StoreError> {
        let dir = self.replica_dir.join("backups").join(&self.replica.document_id);
        fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        let path = dir.join(format!("{}.balancecraft", now.0));
        fs::write(&path, bytes).map_err(io_err(&path))?;
        let mut all: Vec<PathBuf> =
            fs::read_dir(&dir).map_err(io_err(&dir))?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        all.sort();
        let excess = all.len().saturating_sub(BACKUPS);
        for old in all.into_iter().take(excess) {
            let _ = fs::remove_file(old);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
