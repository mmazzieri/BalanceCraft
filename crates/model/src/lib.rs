//! Data model shared with Cerqua (format v1): see docs/plan.md, "Formato del file (v1)".
//!
//! The types mirror Cerqua's `AppData` (mobile/src/types/index.ts, Cerqua 1.46.0)
//! field by field, with the same JSON names. Reading is tolerant, as Cerqua's own
//! backups require: `children` may be missing or `null`, `_meta` may be missing,
//! numbers may be integers or decimals. Every entity keeps the fields it does not
//! know in `extra`, so they survive a read-write cycle.
#![forbid(unsafe_code)]

mod num;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub use num::Num;

/// Fields an entity does not know, kept as they were read.
pub type Extra = Map<String, Value>;

fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// Per-entity sync metadata for the field-level last-writer-wins merge.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EntityMeta {
    /// Field name → last-updated timestamp (ms since the epoch).
    #[serde(default, deserialize_with = "null_as_default", skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Num>,
    /// Last time the parent or the position among siblings changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure: Option<Num>,
    /// Tombstone timestamp: the entity is deleted when set.
    #[serde(rename = "deletedAt", default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<Num>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Inherited settings: system default → global → area → task and its ancestors.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingValues {
    /// Warning, in working days, before a deadline turns yellow.
    #[serde(rename = "advanceDays", default, skip_serializing_if = "Option::is_none")]
    pub advance_days: Option<Num>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GlobalSettings {
    #[serde(flatten)]
    pub values: SettingValues,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<EntityMeta>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<Num>,
    /// Local date-time "YYYY-MM-DDTHH:MM".
    #[serde(rename = "dueDate", default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    /// Local date-time "YYYY-MM-DDTHH:MM": hidden from Today before it.
    #[serde(rename = "startDate", default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Ids of prerequisite leaf tasks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereqs: Option<Vec<String>>,
    #[serde(flatten)]
    pub settings: SettingValues,
    #[serde(default, deserialize_with = "null_as_default")]
    pub children: Vec<TaskNode>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<EntityMeta>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<Num>,
    /// Free one-line status of the area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(rename = "nextActionText", default, skip_serializing_if = "Option::is_none")]
    pub next_action_text: Option<String>,
    #[serde(rename = "nextActionTaskId", default, skip_serializing_if = "Option::is_none")]
    pub next_action_task_id: Option<String>,
    #[serde(flatten)]
    pub settings: SettingValues,
    #[serde(default, deserialize_with = "null_as_default")]
    pub children: Vec<TaskNode>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<EntityMeta>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A completion, or a task put on hold (`waiting`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    #[serde(rename = "taskId", default)]
    pub task_id: String,
    #[serde(rename = "areaId", default)]
    pub area_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "completedAt", default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// Weight of the task when it was completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<bool>,
    #[serde(rename = "waitingFor", default, skip_serializing_if = "Option::is_none")]
    pub waiting_for: Option<String>,
    #[serde(rename = "createdAt", default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<EntityMeta>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Where an area log entry comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionSource {
    Task,
    #[default]
    Manual,
}

/// Area log entry: from a completed task or written by hand.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AreaAction {
    pub id: String,
    #[serde(rename = "areaId", default)]
    pub area_id: String,
    #[serde(default)]
    pub text: String,
    /// ISO date-time of the action.
    #[serde(default)]
    pub at: String,
    #[serde(default)]
    pub source: ActionSource,
    #[serde(rename = "taskId", default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<EntityMeta>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// The whole document: Cerqua's `AppData`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppData {
    #[serde(default, deserialize_with = "null_as_default")]
    pub areas: Vec<Area>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub history: Vec<HistoryEntry>,
    #[serde(rename = "areaActions", default, skip_serializing_if = "Option::is_none")]
    pub area_actions: Option<Vec<AreaAction>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<GlobalSettings>,
    /// Only in Cerqua backups; a `.balancecraft` file keeps it in `savedAt`.
    #[serde(rename = "_savedAt", default, skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<Num>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[cfg(test)]
mod tests;
