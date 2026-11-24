//! BalanceCraft's commands for the phone apps.
//!
//! The apps call one function, [`call`], with a request in JSON and get an answer in JSON:
//! `{"ok": …}` or `{"error": "…"}`. A phone reaches the shared document only through the system, so
//! it keeps a **mirror** of it in its own storage: the commands work on the mirror, with a replica
//! and a merge as on a computer, and the phone carries the mirror to the document and back. The
//! requests are in [`Request`]; `docs/mobile.md` says how they are used.
//!
//! Only the modules that make [`call`] reachable from Java (`android`) and from C and Swift (`c`)
//! use `unsafe`.

#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
pub mod c;

use balancecraft_commands::{Command, Session};
use balancecraft_engine::Clock;
use serde::Deserialize;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

const GENERATOR: &str = concat!("BalanceCraft Mobile ", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Whether a text is a document, and its id.
    Check { text: String },
    /// Runs a command on the mirror, after merging it with the replica in `state`.
    Run { mirror: PathBuf, state: PathBuf, command: Command },
    /// Makes an empty document at the mirror's place.
    New { mirror: PathBuf, state: PathBuf },
}

fn run(request: Request) -> Result<Value, String> {
    let clock = Clock::local().ok_or("the phone's clock cannot be read")?;
    let text = |e: balancecraft_commands::Error| e.to_string();
    match request {
        Request::Check { text } => {
            let (doc, origin) = balancecraft_file::read(text.as_bytes(), GENERATOR, "").map_err(|e| e.to_string())?;
            match origin {
                balancecraft_file::Origin::BalanceCraft => Ok(json!({ "document": doc.document_id })),
                balancecraft_file::Origin::CerquaBackup => Err("the file is not a BalanceCraft document".to_owned()),
            }
        }
        Request::Run { mirror, state, command } => {
            Session::new(&mirror, &state, GENERATOR, None).map_err(text)?.run(command, &clock).map_err(text)
        }
        Request::New { mirror, state } => {
            let session = Session::new(&mirror, &state, GENERATOR, None).map_err(text)?;
            session.create(clock.now_ms()).map(|()| Value::Null).map_err(text)
        }
    }
}

/// Runs one request. Never panics: a request that cannot be read, or a fault inside, comes back
/// as `{"error": …}`.
pub fn call(input: &str) -> String {
    let answer = catch_unwind(AssertUnwindSafe(|| {
        serde_json::from_str(input).map_err(|e| format!("the request could not be read: {e}")).and_then(run)
    }))
    .unwrap_or_else(|_| Err("BalanceCraft's engine stopped on an internal error".to_owned()));
    let value = match answer {
        Ok(v) => json!({ "ok": v }),
        Err(e) => json!({ "error": e }),
    };
    serde_json::to_string(&value).unwrap_or_else(|_| "{\"error\":\"the answer could not be written\"}".to_owned())
}

#[cfg(test)]
mod tests;
