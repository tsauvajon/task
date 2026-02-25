use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use crate::runtime::process::ProcessRunner;

static NIX_GIT_BINARY: OnceLock<Result<PathBuf, String>> = OnceLock::new();

pub(super) fn run_git_capture(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let git_binary = nix_git_binary()?;
    ProcessRunner.run_capture(git_binary.as_os_str(), args, cwd)
}

pub(super) fn run_git_status(args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
    let git_binary = nix_git_binary()?;
    ProcessRunner.run_status(git_binary.as_os_str(), args, cwd)
}

fn nix_git_binary() -> Result<&'static PathBuf, String> {
    let resolved = NIX_GIT_BINARY.get_or_init(resolve_nix_git_binary);
    match resolved {
        Ok(path) => Ok(path),
        Err(error) => Err(error.clone()),
    }
}

fn resolve_nix_git_binary() -> Result<PathBuf, String> {
    let output = Command::new("nix")
        .args(["build", "--no-link", "--print-out-paths", "nixpkgs#git"])
        .output()
        .map_err(|error| format!("Could not resolve nix git package: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("command failed with status {}", output.status)
        } else {
            stderr
        };
        return Err(format!("Could not resolve nix git package: {detail}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let store_path = parse_nix_store_path(&stdout)?;

    let binary = PathBuf::from(store_path).join("bin/git");
    if !binary.is_file() {
        return Err(format!(
            "Could not resolve nix git binary at {}",
            binary.display()
        ));
    }

    Ok(binary)
}

fn parse_nix_store_path(stdout: &str) -> Result<&str, String> {
    stdout
        .lines()
        .find_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                None
            } else {
                Some(line)
            }
        })
        .ok_or_else(|| "Could not resolve nix git package: empty output".to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_nix_store_path;

    #[test]
    fn parse_nix_store_path_returns_first_non_empty_line() {
        let output = "\n  /nix/store/abc-git \n/nix/store/other\n";
        let parsed = parse_nix_store_path(output).expect("store path");
        assert_eq!(parsed, "/nix/store/abc-git");
    }

    #[test]
    fn parse_nix_store_path_rejects_empty_output() {
        assert!(parse_nix_store_path("\n\n").is_err());
    }
}
