//! BalanceCraft commands: one serializable layer over a document, for the command line, the MCP
//! server and any other front end without a window.
//!
//! A change is one command of `balancecraft_engine::commands`; here are only the ids of new
//! entities, the checks on what a caller typed, and the sync round around it. Every run opens the
//! document, so the app and the command line can have it open together: each keeps its own replica
//! and they meet in the merge.
#![forbid(unsafe_code)]

pub mod confine;
mod view;

use balancecraft_engine::commands::{AreaChange, NewArea, NewTask, TaskChange};
use balancecraft_engine::settings::{ADVANCE_DAYS_MAX, ADVANCE_DAYS_MIN};
use balancecraft_engine::tree::{
    MAX_DEPTH, children_of, find_visible_area, find_visible_task, fits_under, height, is_visible_parent,
};
use balancecraft_engine::{Clock, Command as Edit, Ctx, apply};
use balancecraft_file::store::{MERGED_DIR, write_atomic};
use balancecraft_file::{EXTENSION, Now, Store, StoreError};
use balancecraft_model::AppData;
use chrono::{NaiveDate, NaiveDateTime, TimeZone};
use confine::{Access, Confinement};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Time of a deadline and of a start given as a day alone, as in the app.
const DUE_TIME: &str = "23:59";
const START_TIME: &str = "00:00";
pub const WEIGHT_MAX: u8 = 100;
/// Attempts to lock a document that another command is using.
const LOCK_TRIES: u32 = 30;
/// Rounds run until the shared file stops changing after a write.
const SETTLE_ROUNDS: u32 = 5;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Outside the folders of a confined process.
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(message.into()))
}

/// A field that is absent (`None`), emptied with `null` (`Some(None)`) or set.
pub type Patch<T> = Option<Option<T>>;

fn patch<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> std::result::Result<Patch<T>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Areas with their tasks, as a tree.
    List {
        /// Only this area.
        #[serde(default)]
        area: Option<String>,
        /// Completed tasks too.
        #[serde(default)]
        all: bool,
    },
    /// The available tasks by priority.
    Today {
        #[serde(default)]
        limit: Option<usize>,
    },
    AddArea {
        name: String,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        weight: Option<u8>,
    },
    EditArea {
        id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        weight: Option<u8>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        next_action: Option<String>,
    },
    /// A task at the end of an area or of another task.
    Add {
        parent: String,
        name: String,
        #[serde(default)]
        weight: Option<u8>,
        #[serde(default)]
        due: Option<String>,
        #[serde(default)]
        start: Option<String>,
        #[serde(default)]
        notes: Option<String>,
        #[serde(default)]
        prereqs: Vec<String>,
        #[serde(default)]
        advance_days: Option<u8>,
    },
    /// Changes only the fields given; `null` empties one.
    Edit {
        id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default, deserialize_with = "patch", skip_serializing_if = "Option::is_none")]
        weight: Patch<u8>,
        #[serde(default, deserialize_with = "patch", skip_serializing_if = "Option::is_none")]
        due: Patch<String>,
        #[serde(default, deserialize_with = "patch", skip_serializing_if = "Option::is_none")]
        start: Patch<String>,
        #[serde(default, deserialize_with = "patch", skip_serializing_if = "Option::is_none")]
        notes: Patch<String>,
        #[serde(default)]
        prereqs: Option<Vec<String>>,
        #[serde(default, deserialize_with = "patch", skip_serializing_if = "Option::is_none")]
        advance_days: Patch<u8>,
    },
    Complete {
        id: String,
    },
    Uncomplete {
        id: String,
    },
    Hold {
        id: String,
        #[serde(default)]
        reason: String,
    },
    Resume {
        id: String,
    },
    Move {
        id: String,
        parent: String,
        #[serde(default)]
        before: Option<String>,
    },
    /// Deletes a task or an area with everything inside.
    Delete {
        id: String,
    },
    /// Writes a copy of the document to a new file.
    Export {
        file: PathBuf,
    },
}

/// A document and where this computer keeps what it knows of it.
#[derive(Debug, Clone)]
pub struct Session {
    document: PathBuf,
    state: PathBuf,
    generator: String,
    confinement: Option<Confinement>,
}

impl Session {
    /// `state` is the folder of the replicas and backups of the command line, not the app's. The
    /// caller chooses it: an agent has no say in it. Confined, the document must be in a write root.
    pub fn new(document: &Path, state: &Path, generator: &str, confinement: Option<Confinement>) -> Result<Self> {
        let document = std::path::absolute(document).unwrap_or_else(|_| document.to_owned());
        let document = match &confinement {
            Some(c) => c.check(&document, Access::Write)?,
            None => document,
        };
        Ok(Self { document, state: state.to_owned(), generator: generator.to_owned(), confinement })
    }

    /// Creates an empty document; a file that exists is not replaced.
    pub fn create(&self, now_ms: i64) -> Result<()> {
        if self.document.extension().is_none_or(|e| e != EXTENSION) {
            return invalid(format!("a document is named *.{EXTENSION}"));
        }
        if self.document.symlink_metadata().is_ok() {
            return invalid(format!("{} exists already", self.document.display()));
        }
        Store::create(&self.document, &self.state, &self.generator, AppData::default(), Now(now_ms))?;
        Ok(())
    }

    /// A sync round reads the conflict copies beside the document and moves them aside.
    fn check_folder(&self) -> Result<()> {
        let Some(c) = &self.confinement else { return Ok(()) };
        c.check(&self.document, Access::Write)?;
        let Some(dir) = self.document.parent() else { return Ok(()) };
        c.check(&dir.join(MERGED_DIR), Access::Write)?;
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == EXTENSION) && confine::has_other_names(&path) {
                return Err(Error::Refused(format!("{}: the file has another name (a hard link)", path.display())));
            }
        }
        Ok(())
    }

    fn open(&self, now: Now) -> Result<Store> {
        self.check_folder()?;
        let mut tries = 0;
        loop {
            match Store::open(&self.document, &self.state, &self.generator, now) {
                Ok((store, _)) => return Ok(store),
                Err(StoreError::AlreadyOpen) if tries < LOCK_TRIES => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Runs a command at the time of `clock`.
    pub fn run<Tz: TimeZone>(&self, command: Command, clock: &Clock<Tz>) -> Result<Value> {
        let now = Now(clock.now_ms());
        let mut store = self.open(now)?;
        match command {
            Command::List { area, all } => {
                if let Some(id) = &area
                    && find_visible_area(&store.data().areas, id).is_none()
                {
                    return invalid(format!("no area has the id {id}"));
                }
                Ok(view::areas(store.data(), area.as_deref(), all))
            }
            Command::Today { limit } => Ok(view::today(store.data(), clock, limit.unwrap_or(usize::MAX))),
            Command::Export { file } => {
                let target = match &self.confinement {
                    Some(c) => c.check(&file, Access::Write)?,
                    None => std::path::absolute(&file).unwrap_or(file),
                };
                if target.symlink_metadata().is_ok() {
                    return invalid(format!("{} exists already", target.display()));
                }
                let bytes = balancecraft_file::read_file(&self.document)
                    .map_err(|source| StoreError::Io { path: self.document.clone(), source })?;
                // A copy with the same id would be taken for a conflict copy of the document.
                let (mut doc, _) = balancecraft_file::read(&bytes, &self.generator, "")
                    .map_err(|source| StoreError::Read { path: self.document.clone(), source })?;
                doc.document_id = balancecraft_file::new_document_id();
                write_atomic(&target, &balancecraft_file::write(&doc)?)?;
                Ok(json!({"exported": target.display().to_string()}))
            }
            change => {
                let (edits, id) = edits(store.data(), change)?;
                let mut data = store.data().clone();
                let mut changed = false;
                for edit in &edits {
                    let ctx = Ctx::new(&data, now.0);
                    if let Some(next) = apply(&data, edit, &ctx) {
                        data = next;
                        changed = true;
                    }
                }
                if changed {
                    store.set_data(data)?;
                    settle(&mut store, now)?;
                }
                Ok(json!({"id": id, "changed": changed}))
            }
        }
    }
}

/// Writes the edit to the shared file and checks that it stayed there: the app may have written the
/// file in the middle of the round, and there is no next round to bring the edit back.
fn settle(store: &mut Store, now: Now) -> Result<()> {
    store.sync(now)?;
    for _ in 0..SETTLE_ROUNDS {
        std::thread::sleep(Duration::from_millis(200));
        if !store.disk_changed() {
            break;
        }
        store.sync(now)?;
    }
    Ok(())
}

fn too_deep<T>() -> Result<T> {
    invalid(format!("a task goes at most {MAX_DEPTH} levels below its area"))
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The engine's commands for a change, and the id of what it concerns.
fn edits(data: &AppData, command: Command) -> Result<(Vec<Edit>, String)> {
    Ok(match command {
        Command::AddArea { name, color, weight } => {
            let id = new_id();
            let area = NewArea {
                name: name_of(&name)?,
                color: color.map(|c| color_of(&c)).transpose()?,
                weight: weight.map(weight_of).transpose()?,
                advance_days: None,
            };
            (vec![Edit::AddArea { id: id.clone(), area }], id)
        }
        Command::EditArea { id, name, color, weight, status, next_action } => {
            if find_visible_area(&data.areas, &id).is_none() {
                return invalid(format!("no area has the id {id}"));
            }
            let mut changes = Vec::new();
            if let Some(v) = name {
                changes.push(AreaChange::Name(name_of(&v)?));
            }
            if let Some(v) = color {
                changes.push(AreaChange::Color(color_of(&v)?));
            }
            if let Some(v) = weight {
                changes.push(AreaChange::Weight(Some(weight_of(v)?)));
            }
            let mut out = vec![Edit::EditArea { id: id.clone(), changes }];
            if let Some(status) = status {
                out.push(Edit::SetAreaStatus { area_id: id.clone(), status });
            }
            if let Some(text) = next_action {
                out.push(Edit::SetAreaNextAction { area_id: id.clone(), text, task_id: None });
            }
            (out, id)
        }
        Command::Add { parent, name, weight, due, start, notes, prereqs, advance_days } => {
            if !is_visible_parent(&data.areas, &parent) {
                return invalid(format!("no area or task has the id {parent}"));
            }
            if !fits_under(&data.areas, &parent, 1) {
                return too_deep();
            }
            let id = new_id();
            let task = NewTask {
                name: name_of(&name)?,
                // As the app's form.
                weight: Some(weight.map(weight_of).transpose()?.unwrap_or(5.0)),
                due_date: due.map(|d| date_of(&d, DUE_TIME)).transpose()?,
                start_date: start.map(|d| date_of(&d, START_TIME)).transpose()?,
                notes: notes.filter(|n| !n.trim().is_empty()),
                prereqs: prereqs_of(data, None, prereqs)?,
                advance_days: advance_days.map(advance_of).transpose()?,
            };
            (vec![Edit::AddTask { parent_id: parent, id: id.clone(), task }], id)
        }
        Command::Edit { id, name, weight, due, start, notes, prereqs, advance_days } => {
            task(data, &id)?;
            let mut changes = Vec::new();
            if let Some(v) = name {
                changes.push(TaskChange::Name(name_of(&v)?));
            }
            if let Some(v) = weight {
                changes.push(TaskChange::Weight(v.map(weight_of).transpose()?));
            }
            if let Some(v) = due {
                changes.push(TaskChange::DueDate(v.map(|d| date_of(&d, DUE_TIME)).transpose()?));
            }
            if let Some(v) = start {
                changes.push(TaskChange::StartDate(v.map(|d| date_of(&d, START_TIME)).transpose()?));
            }
            if let Some(v) = notes {
                changes.push(TaskChange::Notes(v.filter(|n| !n.trim().is_empty())));
            }
            if let Some(v) = prereqs {
                changes.push(TaskChange::Prereqs(prereqs_of(data, Some(&id), v)?));
            }
            if let Some(v) = advance_days {
                changes.push(TaskChange::AdvanceDays(v.map(advance_of).transpose()?));
            }
            (vec![Edit::EditTask { id: id.clone(), changes }], id)
        }
        Command::Complete { id } => {
            task(data, &id)?;
            if view::completed(data, &id) {
                return invalid(format!("the task {id} is completed already"));
            }
            (vec![Edit::CompleteTask { id: id.clone(), entry_id: new_id(), action_id: new_id() }], id)
        }
        Command::Uncomplete { id } => {
            task(data, &id)?;
            (vec![Edit::Uncomplete { id: id.clone() }], id)
        }
        Command::Hold { id, reason } => {
            task(data, &id)?;
            if view::completed(data, &id) {
                return invalid(format!("the task {id} is completed"));
            }
            (vec![Edit::PutOnHold { id: id.clone(), entry_id: new_id(), reason }], id)
        }
        Command::Resume { id } => {
            task(data, &id)?;
            (vec![Edit::Resume { id: id.clone() }], id)
        }
        Command::Move { id, parent, before } => {
            let moving = task(data, &id)?;
            if !is_visible_parent(&data.areas, &parent) {
                return invalid(format!("no area or task has the id {parent}"));
            }
            if balancecraft_engine::tree::search(moving, &parent).is_some() {
                return invalid("a task cannot be moved inside itself");
            }
            let reorder = children_of(&data.areas, &parent).is_some_and(|c| c.iter().any(|n| n.id == id));
            if !reorder && !fits_under(&data.areas, &parent, height(moving)) {
                return too_deep();
            }
            (vec![Edit::MoveTask { id: id.clone(), new_parent_id: parent, before }], id)
        }
        Command::Delete { id } => {
            let edit = if find_visible_area(&data.areas, &id).is_some() {
                Edit::DeleteArea { id: id.clone() }
            } else {
                task(data, &id)?;
                Edit::DeleteTask { id: id.clone() }
            };
            (vec![edit], id)
        }
        Command::List { .. } | Command::Today { .. } | Command::Export { .. } => (Vec::new(), String::new()),
    })
}

fn task<'a>(data: &'a AppData, id: &str) -> Result<&'a balancecraft_model::TaskNode> {
    match find_visible_task(&data.areas, id) {
        Some((task, _)) => Ok(task),
        None => invalid(format!("no task has the id {id}")),
    }
}

fn name_of(s: &str) -> Result<String> {
    let s = s.trim();
    if s.is_empty() { invalid("the name is blank") } else { Ok(s.to_owned()) }
}

fn color_of(s: &str) -> Result<String> {
    match s.strip_prefix('#') {
        Some(hex) if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(s.to_owned()),
        _ => invalid("a colour is #RRGGBB"),
    }
}

fn weight_of(v: u8) -> Result<f64> {
    if (1..=WEIGHT_MAX).contains(&v) {
        Ok(f64::from(v))
    } else {
        invalid(format!("a weight goes from 1 to {WEIGHT_MAX}"))
    }
}

fn advance_of(v: u8) -> Result<f64> {
    if (ADVANCE_DAYS_MIN..=ADVANCE_DAYS_MAX).contains(&i64::from(v)) {
        Ok(f64::from(v))
    } else {
        invalid(format!("the warning goes from {ADVANCE_DAYS_MIN} to {ADVANCE_DAYS_MAX} days"))
    }
}

fn date_of(s: &str, default_time: &str) -> Result<String> {
    let s = s.trim();
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(format!("{}T{default_time}", d.format("%Y-%m-%d")));
    }
    for format in ["%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, format) {
            return Ok(dt.format("%Y-%m-%dT%H:%M").to_string());
        }
    }
    invalid("a date is YYYY-MM-DD or YYYY-MM-DDTHH:MM")
}

fn prereqs_of(data: &AppData, own: Option<&str>, ids: Vec<String>) -> Result<Vec<String>> {
    for id in &ids {
        task(data, id)?;
        if own == Some(id) {
            return invalid("a task cannot be its own prerequisite");
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests;
