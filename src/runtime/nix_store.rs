/// Resolves Nix store paths for managed tools using `nix path-info`, then
/// caches the result for the lifetime of the process.
///
/// This avoids the full `nix run` startup cost (flake evaluation + process
/// spawn overhead) on every tool invocation: instead we pay the `nix path-info`
/// cost once per tool per process, then execute the store binary directly.
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use crate::{
    error::{Error, Result},
    runtime::process::ManagedTool,
};

/// A lazily-resolved Nix store binary path, cached in a `OnceLock`.
///
/// Construct one as a `static` per tool, then call `capture` / `status`:
///
/// ```ignore
/// static GIT: NixRunner = NixRunner::new(ManagedTool::Git);
/// GIT.capture(&["status"], None)?;
/// ```
pub struct NixRunner {
    tool: ManagedTool,
    binary: OnceLock<Result<PathBuf>>,
}

impl NixRunner {
    pub const fn new(tool: ManagedTool) -> Self {
        Self {
            tool,
            binary: OnceLock::new(),
        }
    }

    fn binary(&self) -> Result<&PathBuf> {
        match self.binary.get_or_init(|| resolve_nix_binary(self.tool)) {
            Ok(path) => Ok(path),
            Err(err) => Err(Error::failed(err.to_string())),
        }
    }

    pub fn capture(&self, args: &[&str], cwd: Option<&Path>) -> Result<String> {
        crate::runtime::process::run_capture(self.binary()?.as_os_str(), args, cwd)
    }

    pub fn status(&self, args: &[&str], cwd: Option<&Path>) -> Result<()> {
        crate::runtime::process::run_status(self.binary()?.as_os_str(), args, cwd)
    }

    pub fn available(&self) -> bool {
        self.binary().is_ok()
    }
}

// NixRunner is used as `static` — safe because OnceLock is Sync.
unsafe impl Sync for NixRunner {}

/// Resolve the primary binary path for a managed tool via `nix path-info`.
///
/// Returns the absolute path to the binary inside the Nix store, e.g.
/// `/nix/store/…-git-2.x/bin/git`.
fn resolve_nix_binary(tool: ManagedTool) -> Result<PathBuf> {
    let package = tool.nix_package();
    let binary_name = tool.binary_name();

    let output = Command::new("nix")
        .args(["path-info", package])
        .output()
        .map_err(|err| Error::failed(format!("Could not resolve nix package {package}: {err}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("command failed with status {}", output.status)
        } else {
            stderr
        };
        return Err(Error::failed(format!(
            "Could not resolve nix package {package}: {detail}"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let store_path = parse_nix_store_path(&stdout, package)?;

    let binary = PathBuf::from(store_path).join("bin").join(binary_name);
    if !binary.is_file() {
        return Err(Error::failed(format!(
            "Could not find binary {binary_name} in nix store path {}",
            binary.display()
        )));
    }

    Ok(binary)
}

fn parse_nix_store_path<'a>(stdout: &'a str, package: &str) -> Result<&'a str> {
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
        .ok_or_else(|| {
            Error::failed(format!(
                "Could not resolve nix package {package}: empty output"
            ))
        })
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
