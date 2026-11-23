//! BalanceCraft engine: the priority algorithm, and later the store commands and
//! the merge. Pure code: no I/O, the clock is a parameter. See docs/plan.md.
#![forbid(unsafe_code)]

pub mod algorithm;
pub mod clock;
pub(crate) mod jsdate;

pub use algorithm::{FlatTask, ShareInfo, Shares, compute_flat_list, compute_shares};
pub use clock::Clock;
