//! The `.balancecraft` file format, version 1: see docs/plan.md, "Formato del file (v1)".
//!
//! A file is UTF-8 JSON: an envelope (`format`, `formatVersion`, `generator`,
//! `documentId`, `savedAt`, `extensions`) around Cerqua's `AppData` in `data`.
//! [`read`] also opens a plain Cerqua backup (`AppData` with `_savedAt`), so a
//! Cerqua user can start from an export.
#![forbid(unsafe_code)]

pub mod conflicts;
mod stale;
pub mod store;

pub use store::{Now, Store, StoreError, SyncReport};

use balancecraft_model::{AppData, Extra};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Value of the `format` field.
pub const FORMAT: &str = "balancecraft";
/// Highest format version this build reads and writes.
pub const FORMAT_VERSION: u32 = 1;
/// File extension, without the dot.
pub const EXTENSION: &str = "balancecraft";
/// Media type of a `.balancecraft` file.
pub const MEDIA_TYPE: &str = "application/vnd.balancecraft+json";

/// Largest file read as a document, a replica or a conflict copy. A document of
/// years of use is a few hundred kilobytes; a bigger file is refused before it
/// is read into memory.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Reads at most [`MAX_FILE_BYTES`] of `path`; a longer file is an error.
pub fn read_file(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(MAX_FILE_BYTES.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(std::io::Error::other(format!("the file is larger than {} MB", MAX_FILE_BYTES / (1024 * 1024))));
    }
    Ok(bytes)
}

/// A `.balancecraft` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub format: String,
    #[serde(rename = "formatVersion")]
    pub format_version: u32,
    /// Program and version that wrote the file, e.g. "BalanceCraft 0.1.0".
    #[serde(default)]
    pub generator: String,
    /// Identity of the document: conflict copies made by sync clients share it.
    #[serde(rename = "documentId")]
    pub document_id: String,
    /// When the file was written, RFC 3339 in UTC.
    #[serde(rename = "savedAt", default)]
    pub saved_at: String,
    pub data: AppData,
    /// Data of other tools, under reverse-domain keys.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extensions: Extra,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Where a document came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A `.balancecraft` file.
    BalanceCraft,
    /// A Cerqua backup, wrapped in a new envelope with a new `documentId`.
    CerquaBackup,
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("the file is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the file is neither a BalanceCraft document nor a Cerqua backup")]
    NotADocument,
    #[error("the file uses format version {found}, this version of BalanceCraft reads up to {FORMAT_VERSION}")]
    TooNew { found: u64 },
}

/// A new, empty document with a fresh `documentId`.
pub fn new_document(generator: &str, saved_at: &str, data: AppData) -> Document {
    Document {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        generator: generator.to_owned(),
        document_id: new_document_id(),
        saved_at: saved_at.to_owned(),
        data,
        extensions: Extra::new(),
        extra: Extra::new(),
    }
}

/// A fresh random document id (UUID v4). "Save as" uses a new one.
pub fn new_document_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Reads a document. A Cerqua backup gets an envelope with a new id and keeps
/// its `_savedAt`, which the legacy migration uses as the baseline timestamp.
pub fn read(bytes: &[u8], generator: &str, now_rfc3339: &str) -> Result<(Document, Origin), ReadError> {
    let value: Value = serde_json::from_slice(bytes)?;
    let Value::Object(obj) = &value else {
        return Err(ReadError::NotADocument);
    };
    match obj.get("format") {
        Some(Value::String(f)) if f == FORMAT => {
            let version = obj.get("formatVersion").and_then(Value::as_u64).ok_or(ReadError::NotADocument)?;
            if version > u64::from(FORMAT_VERSION) {
                return Err(ReadError::TooNew { found: version });
            }
            Ok((serde_json::from_value(value)?, Origin::BalanceCraft))
        }
        Some(_) => Err(ReadError::NotADocument),
        None if obj.contains_key("areas") => {
            let data: AppData = serde_json::from_value(value)?;
            Ok((new_document(generator, now_rfc3339, data), Origin::CerquaBackup))
        }
        None => Err(ReadError::NotADocument),
    }
}

/// Writes a document: JSON indented by two spaces, fields in a stable order,
/// final newline. The same document always gives the same bytes. A document
/// that [`read`] would refuse (nested too deep) is an error, not a file that
/// no longer opens.
pub fn write(doc: &Document) -> Result<Vec<u8>, serde_json::Error> {
    let mut out = serde_json::to_vec_pretty(doc)?;
    serde_json::from_slice::<Value>(&out)?;
    out.push(b'\n');
    fits(&out)?;
    Ok(out)
}

/// Refuses bytes that [`read_file`] would refuse for their size.
pub(crate) fn fits(bytes: &[u8]) -> Result<(), serde_json::Error> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(serde::ser::Error::custom(format!(
            "the document would be larger than {} MB",
            MAX_FILE_BYTES / (1024 * 1024)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
