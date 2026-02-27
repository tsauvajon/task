use std::path::Path;

use crate::{error::Result, runtime::process};

pub fn run_checks(path: &Path) -> Result<()> {
    run_cargo_command(path, &["fmt", "--all"], DyldLibraryPathMode::Preserve)?;
    run_cargo_command(
        path,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        DyldLibraryPathMode::Unset,
    )?;
    run_cargo_command(
        path,
        &["test", "--workspace", "--all-features"],
        DyldLibraryPathMode::Unset,
    )
}

pub fn run_coverage(path: &Path) -> Result<()> {
    run_cargo_command(
        path,
        &[
            "llvm-cov",
            "--workspace",
            "--all-features",
            "--summary-only",
        ],
        DyldLibraryPathMode::Unset,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DyldLibraryPathMode {
    Preserve,
    Unset,
}

fn run_cargo_command(
    path: &Path,
    cargo_args: &[&str],
    dyld_library_path_mode: DyldLibraryPathMode,
) -> Result<()> {
    let args = cargo_command(cargo_args, dyld_library_path_mode);
    process::run_status("nix", &args, Some(path))
}

fn cargo_command<'a>(
    cargo_args: &[&'a str],
    dyld_library_path_mode: DyldLibraryPathMode,
) -> Vec<&'a str> {
    let mut args = vec!["develop", "-c"];
    match dyld_library_path_mode {
        DyldLibraryPathMode::Preserve => args.push("cargo"),
        DyldLibraryPathMode::Unset => {
            args.extend_from_slice(&["env", "-u", "DYLD_LIBRARY_PATH", "cargo"])
        }
    }
    args.extend_from_slice(cargo_args);
    args
}

#[cfg(test)]
mod tests {
    use super::{DyldLibraryPathMode, cargo_command};

    mod cargo_command {
        use super::*;

        #[test]
        fn uses_nix_develop_prefix() {
            let args = cargo_command(&["fmt", "--all"], DyldLibraryPathMode::Preserve);
            assert_eq!(args, vec!["develop", "-c", "cargo", "fmt", "--all"]);
        }

        #[test]
        fn starts_with_develop_c() {
            let args = cargo_command(&["test"], DyldLibraryPathMode::Preserve);
            assert_eq!(args[0], "develop");
            assert_eq!(args[1], "-c");
        }

        #[test]
        fn ends_with_cargo_args() {
            let args = cargo_command(&["test", "--workspace"], DyldLibraryPathMode::Preserve);
            let last_two: Vec<&&str> = args.iter().rev().take(2).collect();
            assert!(args.contains(&"test"), "should contain 'test' subcommand");
            assert!(
                args.contains(&"--workspace"),
                "should contain --workspace flag"
            );
            assert_eq!(args[args.len() - 2], "test");
            assert_eq!(args[args.len() - 1], "--workspace");
            let _ = last_two;
        }

        #[test]
        fn unsets_dyld_library_path_when_requested() {
            let args = cargo_command(&["test", "--workspace"], DyldLibraryPathMode::Unset);
            assert_eq!(
                args,
                vec![
                    "develop",
                    "-c",
                    "env",
                    "-u",
                    "DYLD_LIBRARY_PATH",
                    "cargo",
                    "test",
                    "--workspace",
                ]
            );
        }

        #[test]
        fn preserve_mode_does_not_unset_dyld() {
            let args = cargo_command(&["build"], DyldLibraryPathMode::Preserve);
            assert!(!args.contains(&"env"));
            assert!(!args.contains(&"-u"));
            assert!(!args.contains(&"DYLD_LIBRARY_PATH"));
        }

        #[test]
        fn unset_mode_includes_env_unset_in_order() {
            let args = cargo_command(&["build"], DyldLibraryPathMode::Unset);
            let env_pos = args
                .iter()
                .position(|&a| a == "env")
                .expect("should have env");
            let u_pos = args
                .iter()
                .position(|&a| a == "-u")
                .expect("should have -u");
            let dyld_pos = args
                .iter()
                .position(|&a| a == "DYLD_LIBRARY_PATH")
                .expect("should have DYLD_LIBRARY_PATH");
            assert!(
                env_pos < u_pos && u_pos < dyld_pos,
                "env -u DYLD_LIBRARY_PATH must appear in order"
            );
        }

        #[test]
        fn supports_coverage_subcommand() {
            let args = cargo_command(
                &["llvm-cov", "--workspace", "--summary-only"],
                DyldLibraryPathMode::Unset,
            );
            assert_eq!(
                args,
                vec![
                    "develop",
                    "-c",
                    "env",
                    "-u",
                    "DYLD_LIBRARY_PATH",
                    "cargo",
                    "llvm-cov",
                    "--workspace",
                    "--summary-only",
                ]
            );
        }
    }
}
