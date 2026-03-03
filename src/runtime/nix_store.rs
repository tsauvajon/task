/// Resolves Nix store paths for managed tools using `nix build --no-link
/// --print-out-paths`, then caches the result for the lifetime of the process.
///
/// This avoids the full `nix run` startup cost (flake evaluation + process
/// spawn overhead) on every tool invocation: instead we pay the `nix build`
/// cost once per tool per process, then execute the store binary directly.
///
/// Unlike `nix path-info`, `nix build` fetches or builds the package when it
/// is not already present in the local store, so the resolved path is always
/// valid.
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

    pub fn status_quiet(&self, args: &[&str], cwd: Option<&Path>) -> Result<()> {
        crate::runtime::process::run_status_quiet(self.binary()?.as_os_str(), args, cwd)
    }

    pub fn available(&self) -> bool {
        self.binary().is_ok()
    }
}

// NixRunner is used as `static` — safe because OnceLock is Sync.
unsafe impl Sync for NixRunner {}

/// Resolve the primary binary path for a managed tool via
/// `nix build --no-link --print-out-paths`.
///
/// Returns the absolute path to the binary inside the Nix store, e.g.
/// `/nix/store/…-git-2.x/bin/git`.
///
/// `nix build` fetches or builds the package when it is not already present,
/// unlike `nix path-info` which only reports store paths and fails when the
/// path has not been realised locally.
fn resolve_nix_binary(tool: ManagedTool) -> Result<PathBuf> {
    let package = tool.nix_package();
    let binary_name = tool.binary_name();

    let output = Command::new("nix")
        .args(["build", package, "--no-link", "--print-out-paths"])
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
    find_nix_binary_path(&stdout, package, binary_name)
}

/// Scan each output path printed by `nix build --print-out-paths` and return
/// the path whose `bin/<binary_name>` exists.
///
/// `nix build` may print multiple store paths for a single package (e.g. the
/// package itself and a separate `-man` output). We need the path that actually
/// owns the binary, not the documentation output.
fn find_nix_binary_path(stdout: &str, package: &str, binary_name: &str) -> Result<PathBuf> {
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(line).join("bin").join(binary_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(Error::failed(format!(
        "Could not find binary {binary_name} in nix build output for {package}"
    )))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::find_nix_binary_path;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("task-rs-nix-store-{name}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            self.0.as_path()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    mod find_nix_binary_path {
        use super::*;

        #[test]
        fn returns_path_containing_the_binary() {
            let dir = TempDir::new("find-binary");
            let bin_dir = dir.path().join("bin");
            fs::create_dir_all(&bin_dir).unwrap();
            fs::write(bin_dir.join("mytool"), "").unwrap();

            let stdout = format!("{}\n", dir.path().display());
            let result =
                find_nix_binary_path(&stdout, "nixpkgs#mytool", "mytool").expect("should find");
            assert_eq!(result, bin_dir.join("mytool"));
        }

        #[test]
        fn skips_paths_without_the_binary() {
            let dir = TempDir::new("find-binary-skip");
            // two paths: first has no bin/mytool, second does
            let no_bin = dir.path().join("no-bin");
            let with_bin = dir.path().join("with-bin");
            fs::create_dir_all(no_bin.join("bin")).unwrap();
            fs::create_dir_all(with_bin.join("bin")).unwrap();
            fs::write(with_bin.join("bin").join("mytool"), "").unwrap();

            let stdout = format!("{}\n{}\n", no_bin.display(), with_bin.display());
            let result =
                find_nix_binary_path(&stdout, "nixpkgs#mytool", "mytool").expect("should find");
            assert_eq!(result, with_bin.join("bin").join("mytool"));
        }

        #[test]
        fn skips_man_output_and_finds_bin() {
            // Mirrors the real tmux case: nix build prints -man path first,
            // then the package path that actually has bin/tmux.
            let dir = TempDir::new("find-binary-man");
            let man_path = dir.path().join("tmux-man");
            let pkg_path = dir.path().join("tmux");
            fs::create_dir_all(man_path.join("share").join("man")).unwrap();
            fs::create_dir_all(pkg_path.join("bin")).unwrap();
            fs::write(pkg_path.join("bin").join("tmux"), "").unwrap();

            let stdout = format!("{}\n{}\n", man_path.display(), pkg_path.display());
            let result =
                find_nix_binary_path(&stdout, "nixpkgs#tmux", "tmux").expect("should find tmux");
            assert_eq!(result, pkg_path.join("bin").join("tmux"));
        }

        #[test]
        fn skips_blank_lines() {
            let dir = TempDir::new("find-binary-blanks");
            let pkg_path = dir.path().join("pkg");
            fs::create_dir_all(pkg_path.join("bin")).unwrap();
            fs::write(pkg_path.join("bin").join("tool"), "").unwrap();

            let stdout = format!("\n\n{}\n\n", pkg_path.display());
            let result =
                find_nix_binary_path(&stdout, "nixpkgs#tool", "tool").expect("should find");
            assert_eq!(result, pkg_path.join("bin").join("tool"));
        }

        #[test]
        fn errors_when_no_path_has_the_binary() {
            let dir = TempDir::new("find-binary-missing");
            let pkg_path = dir.path().join("pkg");
            fs::create_dir_all(pkg_path.join("bin")).unwrap();
            // binary not created

            let stdout = format!("{}\n", pkg_path.display());
            let err = find_nix_binary_path(&stdout, "nixpkgs#mytool", "mytool").unwrap_err();
            assert!(
                err.to_string().contains("mytool"),
                "error should mention binary: {err}"
            );
            assert!(
                err.to_string().contains("nixpkgs#mytool"),
                "error should mention package: {err}"
            );
        }

        #[test]
        fn errors_on_empty_output() {
            let err = find_nix_binary_path("", "nixpkgs#git", "git").unwrap_err();
            assert!(
                err.to_string().contains("git"),
                "error should mention binary: {err}"
            );
        }

        #[test]
        fn errors_on_blank_only_output() {
            let err = find_nix_binary_path("\n\n\n", "nixpkgs#git", "git").unwrap_err();
            assert!(
                err.to_string().contains("git"),
                "error should mention binary: {err}"
            );
        }
    }
}
