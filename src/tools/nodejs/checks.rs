use std::path::Path;

use super::runtime::{Runner, corepack_status, pnpm_status, resolve_runner};
use crate::error::Result;

type RunFn = fn(&[&str], Option<&Path>) -> Result<()>;

pub fn run_project_checks(path: &Path) -> Result<bool> {
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

fn quality_commands(runner: Runner) -> (&'static RunFn, Vec<Vec<&'static str>>) {
    match runner {
        Runner::Pnpm => (
            &(pnpm_status as RunFn),
            vec![
                vec!["run", "lint", "--if-present"],
                vec!["run", "check", "--if-present"],
                vec!["run", "test", "--if-present"],
                vec!["run", "build", "--if-present"],
            ],
        ),
        Runner::Corepack => (
            &(corepack_status as RunFn),
            vec![
                vec!["pnpm", "run", "lint", "--if-present"],
                vec!["pnpm", "run", "check", "--if-present"],
                vec!["pnpm", "run", "test", "--if-present"],
                vec!["pnpm", "run", "build", "--if-present"],
            ],
        ),
    }
}

fn run_quality_commands(runner: Runner, path: &Path) -> Result<()> {
    let (run_cmd, commands) = quality_commands(runner);
    for args in &commands {
        run_cmd(args, Some(path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Runner, quality_commands};

    mod quality_commands {
        use super::*;

        #[test]
        fn pnpm_has_four_entries() {
            let (_, commands) = quality_commands(Runner::Pnpm);
            assert_eq!(commands.len(), 4);
        }

        #[test]
        fn corepack_has_four_entries() {
            let (_, commands) = quality_commands(Runner::Corepack);
            assert_eq!(commands.len(), 4);
        }

        #[test]
        fn pnpm_all_use_run_subcommand() {
            let (_, commands) = quality_commands(Runner::Pnpm);
            for cmd in &commands {
                assert_eq!(cmd[0], "run", "pnpm commands should start with 'run'");
            }
        }

        #[test]
        fn corepack_all_start_with_pnpm() {
            let (_, commands) = quality_commands(Runner::Corepack);
            for cmd in &commands {
                assert_eq!(cmd[0], "pnpm", "corepack commands should start with 'pnpm'");
            }
        }

        #[test]
        fn pnpm_all_end_with_if_present() {
            let (_, commands) = quality_commands(Runner::Pnpm);
            for cmd in &commands {
                assert_eq!(
                    cmd.last(),
                    Some(&"--if-present"),
                    "all pnpm quality commands should end with --if-present"
                );
            }
        }

        #[test]
        fn corepack_all_end_with_if_present() {
            let (_, commands) = quality_commands(Runner::Corepack);
            for cmd in &commands {
                assert_eq!(
                    cmd.last(),
                    Some(&"--if-present"),
                    "all corepack quality commands should end with --if-present"
                );
            }
        }

        #[test]
        fn pnpm_includes_lint_check_test_build() {
            let (_, commands) = quality_commands(Runner::Pnpm);
            let scripts: Vec<&str> = commands.iter().map(|c| c[1]).collect();
            assert!(scripts.contains(&"lint"));
            assert!(scripts.contains(&"check"));
            assert!(scripts.contains(&"test"));
            assert!(scripts.contains(&"build"));
        }

        #[test]
        fn corepack_includes_lint_check_test_build() {
            let (_, commands) = quality_commands(Runner::Corepack);
            let scripts: Vec<&str> = commands.iter().map(|c| c[2]).collect();
            assert!(scripts.contains(&"lint"));
            assert!(scripts.contains(&"check"));
            assert!(scripts.contains(&"test"));
            assert!(scripts.contains(&"build"));
        }
    }
}
