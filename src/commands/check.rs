use std::{
    env,
    path::{Path, PathBuf},
};

use crate::{
    runtime::environment::RuntimeEnvironment,
    tools::{nodejs, rust},
};

pub fn run(context: &RuntimeEnvironment, worktree_path: Option<&str>) -> Result<(), String> {
    let path = resolve_check_path(worktree_path);
    if !path.is_dir() {
        return Err(format!("Path not found: {}", path.display()));
    }

    let mut checked = false;
    if path.join("Cargo.toml").exists() {
        checked = true;
        run_rust_checks(context, &path)?;
    }

    if path.join("package.json").exists() {
        checked = true;
        run_js_checks(context, &path)?;
    }

    if !checked {
        context.warn("No Cargo.toml or package.json found. Nothing to run.");
    }

    Ok(())
}

fn resolve_check_path(worktree_path: Option<&str>) -> PathBuf {
    worktree_path
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn run_rust_checks(context: &RuntimeEnvironment, path: &Path) -> Result<(), String> {
    context.log("Running Rust checks");
    ensure_nix_available(context.command_exists("nix"))?;
    rust::run_checks(context.process(), path)
}

fn ensure_nix_available(nix_available: bool) -> Result<(), String> {
    if nix_available {
        return Ok(());
    }

    Err("nix is required for Rust checks. Install nix and retry 'task check'.".to_string())
}

fn run_js_checks(context: &RuntimeEnvironment, path: &Path) -> Result<(), String> {
    context.log("Running JS checks");

    if !nodejs::checks::run_project_checks(context.process(), path)? {
        context.warn("pnpm/corepack not found. Skipping JS checks.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ensure_nix_available;

    #[test]
    fn ensure_nix_available_allows_checks_when_present() {
        ensure_nix_available(true).expect("nix should be available");
    }

    #[test]
    fn ensure_nix_available_rejects_when_missing() {
        let error = ensure_nix_available(false).expect_err("error");
        assert!(error.contains("nix is required"));
    }
}
