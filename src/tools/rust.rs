use std::path::Path;

use crate::runtime::process::ProcessRunner;

pub fn run_checks(process: ProcessRunner, path: &Path) -> Result<(), String> {
    process.run_status("cargo", &["fmt", "--all", "--check"], Some(path))?;
    process.run_status(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        Some(path),
    )?;
    process.run_status(
        "cargo",
        &["test", "--workspace", "--all-features"],
        Some(path),
    )
}
