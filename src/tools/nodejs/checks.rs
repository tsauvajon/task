use std::path::Path;

use super::{
    runner::{run_corepack_status, run_pnpm_status},
    runtime::{corepack_available, enable_corepack, resolve_runner, Runner},
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
            if run_pnpm_status(&["install", "--frozen-lockfile"], Some(path)).is_err() {
                run_pnpm_status(&["install"], Some(path))?;
            }
        }
        Runner::Corepack => {
            if run_corepack_status(&["pnpm", "install", "--frozen-lockfile"], Some(path)).is_err() {
                run_corepack_status(&["pnpm", "install"], Some(path))?;
            }
        }
    }
    Ok(())
}

fn run_quality_commands(runner: Runner, path: &Path) -> Result<()> {
    let (run_cmd, commands): (RunFn, &[&[&str]]) = match runner {
        Runner::Pnpm => (
            run_pnpm_status,
            &[
                &["run", "lint", "--if-present"],
                &["run", "check", "--if-present"],
                &["run", "test", "--if-present"],
                &["run", "build", "--if-present"],
            ],
        ),
        Runner::Corepack => (
            run_corepack_status,
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
