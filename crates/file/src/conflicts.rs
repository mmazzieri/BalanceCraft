//! Conflict copies made by sync clients next to the shared file.
//!
//! | Client | Copy of `Life.balancecraft` |
//! |---|---|
//! | OneDrive | `Life-PCNAME.balancecraft` |
//! | Google Drive | `Life (1).balancecraft` |
//! | Dropbox, Nextcloud | `Life (… conflicted copy …).balancecraft` |
//! | Syncthing | `Life.sync-conflict-20261123-101010-ABCDEFG.balancecraft` |
//!
//! A file is absorbed only if its name follows one of these patterns **and** it
//! has the same `documentId`. A file with the same id and another name is
//! probably a copy made by hand: it is reported, not absorbed.

use crate::store::{MERGED_DIR, StoreError};
use crate::{Document, EXTENSION, Origin, read};
use std::fs;
use std::path::{Path, PathBuf};

fn is_conflict_name(middle: &str) -> bool {
    middle.starts_with('-') && middle.len() > 1
        || middle.starts_with(" (") && middle.ends_with(')')
        || middle.starts_with(".sync-conflict-")
}

/// Conflict copies of `shared` with `document_id`, by save time and name, and
/// the other files with the same id.
pub fn find(shared: &Path, document_id: &str, generator: &str) -> (Vec<(PathBuf, Document)>, Vec<PathBuf>) {
    let (Some(dir), Some(name)) = (shared.parent(), shared.file_name().and_then(|n| n.to_str())) else {
        return (Vec::new(), Vec::new());
    };
    let suffix = format!(".{EXTENSION}");
    let Some(stem) = name.strip_suffix(&suffix) else { return (Vec::new(), Vec::new()) };
    let Ok(entries) = fs::read_dir(if dir.as_os_str().is_empty() { Path::new(".") } else { dir }) else {
        return (Vec::new(), Vec::new());
    };
    let mut copies = Vec::new();
    let mut manual = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(other) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if other == name || other.starts_with(".~") || !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let Some(middle) = other.strip_suffix(&suffix).and_then(|o| o.strip_prefix(stem)) else { continue };
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok((doc, Origin::BalanceCraft)) = read(&bytes, generator, "") else { continue };
        if doc.document_id != document_id {
            continue;
        }
        if is_conflict_name(middle) {
            copies.push((path, doc));
        } else {
            manual.push(path);
        }
    }
    copies.sort_by(|(pa, a), (pb, b)| a.saved_at.cmp(&b.saved_at).then_with(|| pa.cmp(pb)));
    manual.sort();
    (copies, manual)
}

/// Moves absorbed copies into [`MERGED_DIR`] next to the shared file, never
/// over an existing file. Returns where they went.
pub fn move_aside(shared: &Path, paths: impl Iterator<Item = PathBuf>) -> Result<Vec<PathBuf>, StoreError> {
    let dir = shared.parent().unwrap_or(Path::new(".")).join(MERGED_DIR);
    fs::create_dir_all(&dir).map_err(|source| StoreError::Io { path: dir.clone(), source })?;
    let mut out = Vec::new();
    for p in paths {
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        let mut target = dir.join(name);
        let mut n = 1;
        while target.exists() {
            target = dir.join(format!("{n}-{name}"));
            n += 1;
        }
        fs::rename(&p, &target).map_err(|source| StoreError::Io { path: p.clone(), source })?;
        out.push(target);
    }
    Ok(out)
}
