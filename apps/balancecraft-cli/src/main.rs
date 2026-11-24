//! BalanceCraft without a window: the document's commands, and an MCP server for agents.
#![forbid(unsafe_code)]

mod mcp;

use balancecraft_commands::confine::Confinement;
use balancecraft_commands::{Command, Patch, Session};
use balancecraft_engine::Clock;
use clap::{Args, Parser, Subcommand};
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const GENERATOR: &str = concat!("BalanceCraft CLI ", env!("CARGO_PKG_VERSION"));

/// Reads and changes a .balancecraft document without a window.
#[derive(Debug, Parser)]
#[command(name = "balancecraft-cli", version)]
struct Cli {
    /// The document
    #[arg(long, env = "BALANCECRAFT_FILE", value_name = "FILE")]
    file: PathBuf,
    /// Folder of this program's replica and backups
    #[arg(long, value_name = "DIR")]
    state_dir: Option<PathBuf>,
    /// Print the whole answer as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Args)]
struct Fields {
    /// 1 to 100
    #[arg(long)]
    weight: Option<u8>,
    /// YYYY-MM-DD or YYYY-MM-DDTHH:MM
    #[arg(long)]
    due: Option<String>,
    /// Hidden from today's list before this date
    #[arg(long)]
    start: Option<String>,
    #[arg(long)]
    notes: Option<String>,
    /// Id of a task to complete first (repeatable)
    #[arg(long = "prereq", value_name = "ID")]
    prereqs: Vec<String>,
    /// Working days of warning before the deadline
    #[arg(long)]
    advance_days: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Field {
    Weight,
    Due,
    Start,
    Notes,
    Prereqs,
    AdvanceDays,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Create an empty document
    New,
    /// Areas and tasks
    List {
        /// Id of one area
        #[arg(long)]
        area: Option<String>,
        /// Completed tasks too
        #[arg(long)]
        all: bool,
    },
    /// Available tasks by priority
    Today {
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Add an area
    AddArea {
        name: String,
        /// #RRGGBB
        #[arg(long)]
        color: Option<String>,
        #[arg(long)]
        weight: Option<u8>,
    },
    /// Change an area
    EditArea {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        color: Option<String>,
        #[arg(long)]
        weight: Option<u8>,
        /// Status in one line; empty removes it
        #[arg(long)]
        status: Option<String>,
        /// Next action; empty removes it
        #[arg(long)]
        next_action: Option<String>,
    },
    /// Add a task to the area or task PARENT
    Add {
        parent: String,
        name: String,
        #[command(flatten)]
        fields: Fields,
    },
    /// Change a task
    Edit {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        fields: Fields,
        /// Field to empty (repeatable)
        #[arg(long, value_enum)]
        clear: Vec<Field>,
    },
    /// Complete a task
    Complete { id: String },
    /// Undo the completion of a task
    Uncomplete { id: String },
    /// Put a task on hold
    Hold {
        id: String,
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Resume a task on hold
    Resume { id: String },
    /// Move a task under the area or task PARENT
    Move {
        id: String,
        parent: String,
        /// Id of the sibling to put it before
        #[arg(long)]
        before: Option<String>,
    },
    /// Delete a task or an area with everything inside
    Delete { id: String },
    /// Write a copy of the document to a new file
    Export { target: PathBuf },
    /// Serve these commands to an agent (MCP on standard input and output)
    Mcp {
        /// Folder the agent may read (repeatable)
        #[arg(long, required = true, value_name = "DIR")]
        automation_read_root: Vec<PathBuf>,
        /// Folder the agent may write in; the document must be inside one (repeatable)
        #[arg(long, required = true, value_name = "DIR")]
        automation_write_root: Vec<PathBuf>,
    },
}

impl Cmd {
    fn command(self) -> Option<Command> {
        Some(match self {
            Cmd::New | Cmd::Mcp { .. } => return None,
            Cmd::List { area, all } => Command::List { area, all },
            Cmd::Today { limit } => Command::Today { limit },
            Cmd::AddArea { name, color, weight } => Command::AddArea { name, color, weight },
            Cmd::EditArea { id, name, color, weight, status, next_action } => {
                Command::EditArea { id, name, color, weight, status, next_action }
            }
            Cmd::Add { parent, name, fields: f } => Command::Add {
                parent,
                name,
                weight: f.weight,
                due: f.due,
                start: f.start,
                notes: f.notes,
                prereqs: f.prereqs,
                advance_days: f.advance_days,
            },
            Cmd::Edit { id, name, fields: f, clear } => {
                fn patch<T>(value: Option<T>, clear: bool) -> Patch<T> {
                    if clear { Some(None) } else { value.map(Some) }
                }
                let cleared = |field| clear.contains(&field);
                Command::Edit {
                    id,
                    name,
                    weight: patch(f.weight, cleared(Field::Weight)),
                    due: patch(f.due, cleared(Field::Due)),
                    start: patch(f.start, cleared(Field::Start)),
                    notes: patch(f.notes, cleared(Field::Notes)),
                    prereqs: (!f.prereqs.is_empty() || cleared(Field::Prereqs)).then_some(f.prereqs),
                    advance_days: patch(f.advance_days, cleared(Field::AdvanceDays)),
                }
            }
            Cmd::Complete { id } => Command::Complete { id },
            Cmd::Uncomplete { id } => Command::Uncomplete { id },
            Cmd::Hold { id, reason } => Command::Hold { id, reason },
            Cmd::Resume { id } => Command::Resume { id },
            Cmd::Move { id, parent, before } => Command::Move { id, parent, before },
            Cmd::Delete { id } => Command::Delete { id },
            Cmd::Export { target } => Command::Export { file: target },
        })
    }
}

/// Beside the app's data folder, not in it: the app locks the documents it has open.
fn default_state_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local").join("share")))
    };
    base.map(|b| b.join("BalanceCraft CLI"))
}

fn clock() -> Result<Clock<chrono::Local>, String> {
    Clock::local().ok_or_else(|| "the computer's clock cannot be read".to_owned())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("balancecraft-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let state = cli.state_dir.or_else(default_state_dir).ok_or("no local data folder: use --state-dir")?;
    let open = |roots| Session::new(&cli.file, &state, GENERATOR, roots).map_err(|e| e.to_string());
    if let Cmd::Mcp { automation_read_root, automation_write_root } = &cli.cmd {
        let roots = Confinement::new(automation_read_root, automation_write_root).map_err(|e| e.to_string())?;
        return mcp::serve(open(Some(roots))?).map_err(|e| e.to_string());
    }
    let session = open(None)?;
    let Some(command) = cli.cmd.command() else {
        return session.create(clock()?.now_ms()).map_err(|e| e.to_string());
    };
    let answer = session.run(command.clone(), &clock()?).map_err(|e| e.to_string())?;
    let text = if cli.json {
        serde_json::to_string_pretty(&answer).map_err(|e| e.to_string())? + "\n"
    } else {
        plain(&command, &answer)
    };
    std::io::stdout().write_all(text.as_bytes()).map_err(|e| e.to_string())
}

fn field(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn items<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    v.get(key).and_then(Value::as_array).into_iter().flatten()
}

/// One line per area or task: id, then the name indented by depth.
fn tree(nodes: &Value, key: &str, depth: usize, out: &mut String) {
    for n in items(nodes, key) {
        out.push_str(&format!("{}\t{}{}", field(n, "id"), "  ".repeat(depth), field(n, "name")));
        for k in ["due", "on_hold", "completed"] {
            if n.get(k).is_some() {
                out.push_str(&format!("  [{k}: {}]", field(n, k)));
            }
        }
        out.push('\n');
        tree(n, "tasks", depth + 1, out);
    }
}

/// The answer for a person.
fn plain(command: &Command, answer: &Value) -> String {
    let mut out = String::new();
    match command {
        Command::List { .. } => tree(answer, "areas", 0, &mut out),
        Command::Today { .. } => {
            for t in items(answer, "tasks") {
                let path: Vec<&str> = items(t, "path").filter_map(Value::as_str).collect();
                out.push_str(&format!("{}\t{}\t{}\n", field(t, "id"), path.join(" › "), field(t, "due")));
            }
        }
        Command::Export { .. } => out = field(answer, "exported") + "\n",
        _ if answer.get("changed") == Some(&Value::Bool(false)) => out = "nothing changed\n".to_owned(),
        _ => out = field(answer, "id") + "\n",
    }
    out
}
