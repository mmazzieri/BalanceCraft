//! BalanceCraft engine: the priority algorithm, the store commands and the
//! merge. Pure code: no I/O, the clock is a parameter. See docs/plan.md.
#![forbid(unsafe_code)]

pub mod algorithm;
pub mod calendar;
pub mod clock;
pub mod commands;
pub mod defaults;
pub(crate) mod jsdate;
pub(crate) mod json;
pub mod merge;
pub mod meta;
pub mod settings;
pub mod stamps;
pub mod tree;

pub use algorithm::{FlatTask, ShareInfo, Shares, compute_flat_list, compute_shares};
pub use clock::Clock;
pub use commands::{Command, Ctx, apply};
pub use merge::{Rules, merge, merge_json};
