use std::path::Path;

use super::runtime::{
    Runner, corepack_available, corepack_status, enable_corepack, pnpm_status, resolve_runner,
};
use crate::error::Result;

type RunFn = fn(&[&str], Option<&Path>) -> Result<()>;

pub fn run_project_checks(path: &Path) -> Result<bool> {
    if corepack_available() {
        let _ = enable_corepack();
    }

    let Some(runner) = resolve_runner() else {
        return Ok(false);
    };

    install_dependencies(runner, path)?;
    run_quality_commands(runner, path)?;
    Ok(true)
}

fn install_dependencies(runner: Runner, path: &Path) -> Result<()> {
    match runner {
        Runner::Pnpm => {
            if pnpm_status(&["install", "--frozen-lockfile"], Some(path)).is_err() {
                pnpm_status(&["install"], Some(path))?;
            }
        }
        Runner::Corepack => {
            if corepack_status(&["pnpm", "install", "--frozen-lockfile"], Some(path)).is_err() {
                corepack_status(&["pnpm", "install"], Some(path))?;
            }
        }
    }
    Ok(())
}

fn run_quality_commands(runner: Runner, path: &Path) -> Result<()> {
    let (run_cmd, commands): (RunFn, &[&[&str]]) = match runner {
        Runner::Pnpm => (
            pnpm_status,
            &[
                &["run", "lint", "--if-present"],
                &["run", "check", "--if-present"],
                &["run", "test", "--if-present"],
                &["run", "build", "--if-present"],
            ],
        ),
        Runner::Corepack => (
            corepack_status,
            &[
                &["pnpm", "run", "lint", "--if-present"],
                &["pnpm", "run", "check", "--if-present"],
                &["pnpm", "run", "test", "--if-present"],
                &["pnpm", "run", "build", "--if-present"],
            ],
        ),
    };

    for args in commands {
        run_cmd(args, Some(path))?;
    }
    Ok(())
}
