use std::env;
use std::path::PathBuf;

pub fn run(worktree_path: Option<&str>) -> Result<(), String> {
    let path = worktree_path
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if !path.is_dir() {
        return Err(format!("Path not found: {}", path.display()));
    }

    let mut checked = false;
    if path.join("Cargo.toml").exists() {
        checked = true;
        super::log("Running Rust checks");
        super::run_status("cargo", &["fmt", "--all", "--check"], Some(&path))?;
        super::run_status(
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
            Some(&path),
        )?;
        super::run_status(
            "cargo",
            &["test", "--workspace", "--all-features"],
            Some(&path),
        )?;
    }

    if path.join("package.json").exists() {
        checked = true;
        super::log("Running JS checks");
        if super::command_exists("corepack") {
            let _ = super::run_status("corepack", &["enable"], None);
        }

        let (tool, install_args): (&str, Vec<&str>) = if super::command_exists("pnpm") {
            ("pnpm", vec!["install", "--frozen-lockfile"])
        } else if super::command_exists("corepack") {
            ("corepack", vec!["pnpm", "install", "--frozen-lockfile"])
        } else {
            super::warn("pnpm/corepack not found. Skipping JS checks.");
            ("", Vec::new())
        };

        if !tool.is_empty() {
            if super::run_status(tool, &install_args, Some(&path)).is_err() {
                let fallback = if tool == "pnpm" {
                    vec!["install"]
                } else {
                    vec!["pnpm", "install"]
                };
                super::run_status(tool, &fallback, Some(&path))?;
            }

            let commands = if tool == "pnpm" {
                vec![
                    vec!["run", "lint", "--if-present"],
                    vec!["run", "check", "--if-present"],
                    vec!["run", "test", "--if-present"],
                    vec!["run", "build", "--if-present"],
                ]
            } else {
                vec![
                    vec!["pnpm", "run", "lint", "--if-present"],
                    vec!["pnpm", "run", "check", "--if-present"],
                    vec!["pnpm", "run", "test", "--if-present"],
                    vec!["pnpm", "run", "build", "--if-present"],
                ]
            };

            for args in commands {
                super::run_status(tool, &args, Some(&path))?;
            }
        }
    }

    if !checked {
        super::warn("No Cargo.toml or package.json found. Nothing to run.");
    }

    Ok(())
}
