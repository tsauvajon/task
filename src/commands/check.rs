use std::env;
use std::path::{Path, PathBuf};

use crate::runtime::environment::RuntimeEnvironment;
use crate::tools::{nodejs, rust};

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
    rust::run_checks(context.process(), path)
}

fn run_js_checks(context: &RuntimeEnvironment, path: &Path) -> Result<(), String> {
    context.log("Running JS checks");

    if !nodejs::checks::run_project_checks(context.process(), path)? {
        context.warn("pnpm/corepack not found. Skipping JS checks.");
    }

    Ok(())
}
