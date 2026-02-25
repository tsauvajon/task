/// Resolves Nix store paths for managed tools using `nix path-info`, then
/// caches the result for the lifetime of the process.
///
/// This avoids the full `nix run` startup cost (flake evaluation + process
/// spawn overhead) on every tool invocation: instead we pay the `nix path-info`
/// cost once per tool per process, then execute the store binary directly.
use std::{
    path::{Path, PathBuf},
    process::Command,
};

use crate::runtime::process::ManagedTool;

/// Resolve the primary binary path for a managed tool via `nix path-info`.
///
/// Returns the absolute path to the binary inside the Nix store, e.g.
/// `/nix/store/…-git-2.x/bin/git`.  The result is **not** cached here —
/// callers are expected to hold it in a `OnceLock` themselves (one per tool)
/// so that the resolution cost is paid at most once per process.
pub fn resolve_nix_binary(tool: ManagedTool) -> Result<PathBuf, String> {
    let package = tool.nix_package();
    let binary_name = tool_binary_name(tool);

    let output = Command::new("nix")
        .args(["path-info", package])
        .output()
        .map_err(|error| format!("Could not resolve nix package {package}: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("command failed with status {}", output.status)
        } else {
            stderr
        };
        return Err(format!("Could not resolve nix package {package}: {detail}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let store_path = parse_nix_store_path(&stdout, package)?;

    let binary = PathBuf::from(store_path).join("bin").join(binary_name);
    if !binary.is_file() {
        return Err(format!(
            "Could not find binary {} in nix store path {}",
            binary_name,
            binary.display()
        ));
    }

    Ok(binary)
}

/// Maps a `ManagedTool` to the binary name it exposes inside `bin/`.
///
/// Most tools match their package name, but a few diverge (e.g. `vscodium`
/// ships `codium`, `asdf-vm` ships `asdf`, `nodejs` ships multiple binaries).
fn tool_binary_name(tool: ManagedTool) -> &'static str {
    match tool {
        ManagedTool::Git => "git",
        ManagedTool::Tmux => "tmux",
        ManagedTool::Codium => "codium",
        ManagedTool::Opencode => "opencode",
        ManagedTool::Direnv => "direnv",
        ManagedTool::Asdf => "asdf",
        ManagedTool::Pnpm => "pnpm",
        ManagedTool::Corepack => "corepack",
        ManagedTool::Node => "node",
    }
}

fn parse_nix_store_path<'a>(stdout: &'a str, package: &str) -> Result<&'a str, String> {
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
        .ok_or_else(|| format!("Could not resolve nix package {package}: empty output"))
}

/// Returns a reference to the resolved binary path, or an error string.
///
/// Intended to be called from a `OnceLock::get_or_init` closure:
///
/// ```ignore
/// static MY_BINARY: OnceLock<Result<PathBuf, String>> = OnceLock::new();
///
/// fn my_binary() -> Result<&'static PathBuf, String> {
///     cached_nix_binary(&MY_BINARY, ManagedTool::Tmux)
/// }
/// ```
pub fn cached_nix_binary(
    lock: &std::sync::OnceLock<Result<PathBuf, String>>,
    tool: ManagedTool,
) -> Result<&PathBuf, String> {
    match lock.get_or_init(|| resolve_nix_binary(tool)) {
        Ok(path) => Ok(path),
        Err(error) => Err(error.clone()),
    }
}

/// Runs a command using a pre-resolved Nix store binary path.
///
/// Passes `args` directly to the binary at `binary_path`, bypassing `nix run`.
pub fn run_nix_binary_capture(
    binary_path: &Path,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String, String> {
    use crate::runtime::process::ProcessRunner;
    ProcessRunner.run_capture(binary_path.as_os_str(), args, cwd)
}

pub fn run_nix_binary_status(
    binary_path: &Path,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<(), String> {
    use crate::runtime::process::ProcessRunner;
    ProcessRunner.run_status(binary_path.as_os_str(), args, cwd)
}

#[cfg(test)]
mod tests {
    use super::parse_nix_store_path;

    #[test]
    fn parse_nix_store_path_returns_first_non_empty_line() {
        let output = "\n  /nix/store/abc-tmux \n/nix/store/other\n";
        let result = parse_nix_store_path(output, "nixpkgs#tmux").expect("store path");
        assert_eq!(result, "/nix/store/abc-tmux");
    }

    #[test]
    fn parse_nix_store_path_rejects_empty_output() {
        assert!(parse_nix_store_path("\n\n", "nixpkgs#tmux").is_err());
    }
}
