use std::env;
use std::path::{Path, PathBuf};

use crate::runtime::RuntimeEnvironment;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsRunner {
    Pnpm,
    Corepack,
}

fn resolve_check_path(worktree_path: Option<&str>) -> PathBuf {
    worktree_path
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn run_rust_checks(context: &RuntimeEnvironment, path: &Path) -> Result<(), String> {
    context.log("Running Rust checks");
    context.run_status("cargo", &["fmt", "--all", "--check"], Some(path))?;
    context.run_status(
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
    context.run_status(
        "cargo",
        &["test", "--workspace", "--all-features"],
        Some(path),
    )
}

fn resolve_js_runner(has_pnpm: bool, has_corepack: bool) -> Option<JsRunner> {
    if has_pnpm {
        return Some(JsRunner::Pnpm);
    }
    if has_corepack {
        return Some(JsRunner::Corepack);
    }
    None
}

fn install_args(runner: JsRunner) -> &'static [&'static str] {
    match runner {
        JsRunner::Pnpm => &["install", "--frozen-lockfile"],
        JsRunner::Corepack => &["pnpm", "install", "--frozen-lockfile"],
    }
}

fn install_fallback_args(runner: JsRunner) -> &'static [&'static str] {
    match runner {
        JsRunner::Pnpm => &["install"],
        JsRunner::Corepack => &["pnpm", "install"],
    }
}

fn script_commands(runner: JsRunner) -> &'static [&'static [&'static str]] {
    match runner {
        JsRunner::Pnpm => &[
            &["run", "lint", "--if-present"],
            &["run", "check", "--if-present"],
            &["run", "test", "--if-present"],
            &["run", "build", "--if-present"],
        ],
        JsRunner::Corepack => &[
            &["pnpm", "run", "lint", "--if-present"],
            &["pnpm", "run", "check", "--if-present"],
            &["pnpm", "run", "test", "--if-present"],
            &["pnpm", "run", "build", "--if-present"],
        ],
    }
}

fn run_js_checks(context: &RuntimeEnvironment, path: &Path) -> Result<(), String> {
    context.log("Running JS checks");
    let has_corepack = context.command_exists("corepack");
    if has_corepack {
        let _ = context.run_status("corepack", &["enable"], None);
    }

    let has_pnpm = context.command_exists("pnpm");
    let Some(runner) = resolve_js_runner(has_pnpm, has_corepack) else {
        context.warn("pnpm/corepack not found. Skipping JS checks.");
        return Ok(());
    };

    let program = match runner {
        JsRunner::Pnpm => "pnpm",
        JsRunner::Corepack => "corepack",
    };

    if context
        .run_status(program, install_args(runner), Some(path))
        .is_err()
    {
        context.run_status(program, install_fallback_args(runner), Some(path))?;
    }

    for args in script_commands(runner) {
        context.run_status(program, args, Some(path))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        JsRunner, install_args, install_fallback_args, resolve_js_runner, script_commands,
    };

    #[test]
    fn resolve_js_runner_prefers_pnpm() {
        assert_eq!(resolve_js_runner(true, true), Some(JsRunner::Pnpm));
    }

    #[test]
    fn resolve_js_runner_uses_corepack_when_pnpm_missing() {
        assert_eq!(resolve_js_runner(false, true), Some(JsRunner::Corepack));
    }

    #[test]
    fn resolve_js_runner_returns_none_when_unavailable() {
        assert_eq!(resolve_js_runner(false, false), None);
    }

    #[test]
    fn install_args_match_runner() {
        assert_eq!(
            install_args(JsRunner::Pnpm),
            ["install", "--frozen-lockfile"]
        );
        assert_eq!(
            install_args(JsRunner::Corepack),
            ["pnpm", "install", "--frozen-lockfile"]
        );
    }

    #[test]
    fn install_fallback_args_match_runner() {
        assert_eq!(install_fallback_args(JsRunner::Pnpm), ["install"]);
        assert_eq!(
            install_fallback_args(JsRunner::Corepack),
            ["pnpm", "install"]
        );
    }

    #[test]
    fn script_commands_include_expected_steps() {
        assert_eq!(
            script_commands(JsRunner::Pnpm),
            [
                ["run", "lint", "--if-present"].as_slice(),
                ["run", "check", "--if-present"].as_slice(),
                ["run", "test", "--if-present"].as_slice(),
                ["run", "build", "--if-present"].as_slice(),
            ]
        );
        assert_eq!(
            script_commands(JsRunner::Corepack),
            [
                ["pnpm", "run", "lint", "--if-present"].as_slice(),
                ["pnpm", "run", "check", "--if-present"].as_slice(),
                ["pnpm", "run", "test", "--if-present"].as_slice(),
                ["pnpm", "run", "build", "--if-present"].as_slice(),
            ]
        );
    }
}
