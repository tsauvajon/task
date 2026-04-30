#![cfg_attr(
    test,
    expect(
        clippy::indexing_slicing,
        clippy::wildcard_enum_match_arm,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "unit tests use direct indexing, unwraps, and panics for concise assertions"
    )
)]

pub mod commands;
pub mod error;
pub mod runtime;
pub mod tools;
pub mod ui;
