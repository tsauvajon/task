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

    #[test]
    fn cargo_command_uses_nix_develop_prefix() {
        let args = cargo_command(&["fmt", "--all"], DyldLibraryPathMode::Preserve);
        assert_eq!(args, vec!["develop", "-c", "cargo", "fmt", "--all"]);
    }

    #[test]
    fn cargo_command_unsets_dyld_library_path_when_requested() {
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
    fn cargo_command_supports_coverage_subcommand() {
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
