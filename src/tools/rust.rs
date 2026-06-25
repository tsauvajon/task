use std::path::Path;

use crate::{error::Result, runtime::process};

pub fn run_coverage(path: &Path) -> Result<()> {
    run_cargo_command(
        path,
        &[
            "llvm-cov",
            "--workspace",
            "--all-features",
            "--summary-only",
        ],
    )
}

fn run_cargo_command(path: &Path, cargo_args: &[&str]) -> Result<()> {
    process::run_status("cargo", cargo_args, Some(path))
}

#[cfg(test)]
mod tests {
    // Nothing to unit-test here: the module is a thin PATH-based wrapper over
    // `cargo`. Behavior is exercised through integration flows.
}
