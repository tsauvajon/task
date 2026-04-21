use std::{env, path::PathBuf};

use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process},
    tools::{nodejs, rust},
};

pub fn run(_env: &RuntimeEnvironment, worktree_path: Option<&str>) -> Result<()> {
    let path = resolve_check_path(worktree_path);
    if !path.is_dir() {
        return Err(Error::not_found(format!(
            "Path not found: {}",
            path.display()
        )));
    }

    let mut checked = false;

    if path.join("Cargo.toml").exists() {
        checked = true;
        process::log("Running Rust checks");
        rust::run_checks(&path)?;
    }

    if path.join("package.json").exists() {
        checked = true;
        process::log("Running JS checks");
        if !nodejs::checks::run_project_checks(&path)? {
            process::warn("pnpm/corepack not found. Skipping JS checks.");
        }
    }

    if !checked {
        process::warn("No Cargo.toml or package.json found. Nothing to run.");
    }

    Ok(())
}

fn resolve_check_path(worktree_path: Option<&str>) -> PathBuf {
    worktree_path
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::resolve_check_path;

    mod resolve_check_path {
        use super::*;

        #[test]
        fn uses_explicit_arg() {
            let path = resolve_check_path(Some("/some/explicit/path"));
            assert_eq!(path, PathBuf::from("/some/explicit/path"));
        }

        #[test]
        fn uses_cwd_when_none() {
            // When no path is given, it should return *something* (current dir or ".")
            // without panicking.
            let path = resolve_check_path(None);
            let s = path.to_string_lossy();
            assert!(!s.is_empty(), "resolved path must not be empty");
        }

        #[test]
        fn converts_relative_arg() {
            let path = resolve_check_path(Some("relative/path"));
            assert_eq!(path, PathBuf::from("relative/path"));
        }
    }
}
