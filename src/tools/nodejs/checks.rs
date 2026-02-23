use std::path::Path;

use crate::runtime::ProcessRunner;

use super::{Runner, corepack_available, enable_corepack, resolve_runner};

pub fn run_project_checks(process: ProcessRunner, path: &Path) -> Result<bool, String> {
    if corepack_available(process) {
        let _ = enable_corepack(process);
    }

    let Some(runner) = resolve_runner(process) else {
        return Ok(false);
    };

    install_dependencies(process, runner, path)?;
    run_quality_commands(process, runner, path)?;
    Ok(true)
}

fn install_dependencies(process: ProcessRunner, runner: Runner, path: &Path) -> Result<(), String> {
    let (program, frozen_args, fallback_args) = match runner {
        Runner::Pnpm => (
            "pnpm",
            &["install", "--frozen-lockfile"][..],
            &["install"][..],
        ),
        Runner::Corepack => (
            "corepack",
            &["pnpm", "install", "--frozen-lockfile"][..],
            &["pnpm", "install"][..],
        ),
    };

    if process
        .run_status(program, frozen_args, Some(path))
        .is_err()
    {
        process.run_status(program, fallback_args, Some(path))?;
    }

    Ok(())
}

fn run_quality_commands(process: ProcessRunner, runner: Runner, path: &Path) -> Result<(), String> {
    let (program, commands): (&str, &[&[&str]]) = match runner {
        Runner::Pnpm => (
            "pnpm",
            &[
                &["run", "lint", "--if-present"],
                &["run", "check", "--if-present"],
                &["run", "test", "--if-present"],
                &["run", "build", "--if-present"],
            ],
        ),
        Runner::Corepack => (
            "corepack",
            &[
                &["pnpm", "run", "lint", "--if-present"],
                &["pnpm", "run", "check", "--if-present"],
                &["pnpm", "run", "test", "--if-present"],
                &["pnpm", "run", "build", "--if-present"],
            ],
        ),
    };

    for args in commands {
        process.run_status(program, args, Some(path))?;
    }

    Ok(())
}
