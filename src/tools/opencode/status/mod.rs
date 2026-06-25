//! Classify the live state of `OpenCode` sessions for a worktree.
//!
//! A snapshot is produced once per refresh (via [`OpenCodeSnapshot`])
//! and reused to classify every visible task row, so the expensive
//! parts — scanning the process list and discovering `OpenCode` DBs —
//! happen exactly once.
//!
//! Rollup across multiple sessions for the same worktree follows the
//! agreed priority, from most to least attention-worthy:
//! `Hung > Busy > Idle > Gone > None`.

pub use classify::OpenCodeState;
pub use snapshot::OpenCodeSnapshot;

mod activity;
mod classify;
mod message;
mod sessions;
mod snapshot;

#[cfg(test)]
mod test_support;

#[cfg(test)]
#[path = "real_cases_tests.rs"]
mod real_cases;
