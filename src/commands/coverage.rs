use std::{env, path::PathBuf};

use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process},
    tools::rust,
};

pub fn run(_env: &RuntimeEnvironment, worktree_path: Option<&str>) -> Result<()> {
    let path = resolve_check_path(worktree_path);
    if !path.is_dir() {
        return Err(Error::not_found(format!(
            "Path not found: {}",
            path.display()
        )));
    }

    if !path.join("Cargo.toml").exists() {
        return Err(Error::not_found(format!(
            "Cargo.toml not found in {}",
            path.display()
        )));
    }

    ensure_nix_available(process::nix_available())?;
    process::log("Running Rust test coverage with cargo-llvm-cov");
    rust::run_coverage(&path)
}

fn resolve_check_path(worktree_path: Option<&str>) -> PathBuf {
    worktree_path
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn ensure_nix_available(nix_available: bool) -> Result<()> {
    if nix_available {
        return Ok(());
    }
    Err(Error::failed(
        "nix is required for Rust coverage. Install nix and retry 'task coverage'.",
    ))
}

#[cfg(test)]
mod tests {
    use super::ensure_nix_available;

    #[test]
    fn ensure_nix_available_allows_coverage_when_present() {
        ensure_nix_available(true).expect("nix should be available");
    }

    #[test]
    fn ensure_nix_available_rejects_when_missing() {
        let err = ensure_nix_available(false).expect_err("error");
        assert!(err.to_string().contains("nix is required"));
    }
}
