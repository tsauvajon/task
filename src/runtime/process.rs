use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Stdio},
};

use owo_colors::OwoColorize;

/// Returns the nixpkgs attribute for a tool, if it should be run via `nix run`.
///
/// Tools mapped here are launched as `nix run nixpkgs#<pkg> -- <tool> <args…>`.
/// Tools not in this map are invoked directly (e.g. `nix` itself, `kill`).
pub fn nix_package_for(tool: &str) -> Option<&'static str> {
    match tool {
        "git" => Some("nixpkgs#git"),
        "tmux" => Some("nixpkgs#tmux"),
        "codium" => Some("nixpkgs#vscodium"),
        "opencode" => Some("nixpkgs#opencode"),
        "direnv" => Some("nixpkgs#direnv"),
        "asdf" => Some("nixpkgs#asdf-vm"),
        "pnpm" => Some("nixpkgs#pnpm"),
        "corepack" | "node" => Some("nixpkgs#nodejs"),
        _ => None,
    }
}

/// Builds the full argument list for launching `<tool> <args…>` via `nix run`.
///
/// Returns `["run", "<nixpkg>", "--", "<tool>", <args…>]`.
pub fn nix_run_args<'a>(nixpkg: &'a str, tool: &'a str, args: &'a [&'a str]) -> Vec<&'a str> {
    let mut nix_args = vec!["run", nixpkg, "--", tool];
    nix_args.extend_from_slice(args);
    nix_args
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessRunner;

impl ProcessRunner {
    /// Returns true if the named tool is available.
    ///
    /// For tools with a nix package mapping, availability is determined by
    /// whether `nix` itself is on PATH (the tool will be fetched via `nix run`
    /// on demand). For unmapped tools and absolute paths, the binary is looked
    /// up directly on PATH / filesystem.
    pub fn command_exists(&self, name: &str) -> bool {
        if name.contains('/') {
            return Path::new(name).exists();
        }

        if nix_package_for(name).is_some() {
            return self.nix_available();
        }

        let path_var = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path_var).any(|dir| dir.join(name).exists())
    }

    /// Returns true if `nix` is available on PATH.
    pub fn nix_available(&self) -> bool {
        let path_var = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path_var).any(|dir| dir.join("nix").exists())
    }

    pub fn run_capture(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<String, String> {
        let program_str = program.as_ref().to_string_lossy();
        if let Some(nixpkg) = nix_package_for(&program_str) {
            let nix_args = nix_run_args(nixpkg, &program_str, args);
            return self.run_capture_raw("nix", &nix_args, cwd);
        }
        self.run_capture_raw(program, args, cwd)
    }

    pub fn run_status(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<(), String> {
        let program_str = program.as_ref().to_string_lossy();
        if let Some(nixpkg) = nix_package_for(&program_str) {
            let nix_args = nix_run_args(nixpkg, &program_str, args);
            return self.run_status_raw("nix", &nix_args, cwd);
        }
        self.run_status_raw(program, args, cwd)
    }

    pub fn spawn_detached(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<(), String> {
        let program_str = program.as_ref().to_string_lossy();
        if let Some(nixpkg) = nix_package_for(&program_str) {
            let nix_args = nix_run_args(nixpkg, &program_str, args);
            return self.spawn_detached_raw("nix", &nix_args, cwd);
        }
        self.spawn_detached_raw(program, args, cwd)
    }

    fn run_capture_raw(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<String, String> {
        let mut command = Command::new(program);
        command.args(args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let output = command.output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() {
                return Err(stderr);
            }
            return Err(format!("command failed with status {}", output.status));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn run_status_raw(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<(), String> {
        let mut command = Command::new(program);
        command.args(args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }

        let status = command.status().map_err(|e| e.to_string())?;
        if status.success() {
            return Ok(());
        }
        Err(format!("command failed with status {status}"))
    }

    fn spawn_detached_raw(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<(), String> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        command.spawn().map(|_| ()).map_err(|e| e.to_string())
    }

    pub fn log(&self, message: &str) {
        println!("{} {}", "==>".bright_blue().bold(), message);
    }

    pub fn warn(&self, message: &str) {
        eprintln!("{} {}", "warning:".yellow().bold(), message);
    }
}

#[cfg(test)]
mod tests {
    use super::{nix_package_for, nix_run_args};

    #[test]
    fn nix_package_for_maps_known_tools() {
        assert_eq!(nix_package_for("git"), Some("nixpkgs#git"));
        assert_eq!(nix_package_for("tmux"), Some("nixpkgs#tmux"));
        assert_eq!(nix_package_for("codium"), Some("nixpkgs#vscodium"));
        assert_eq!(nix_package_for("opencode"), Some("nixpkgs#opencode"));
        assert_eq!(nix_package_for("direnv"), Some("nixpkgs#direnv"));
        assert_eq!(nix_package_for("asdf"), Some("nixpkgs#asdf-vm"));
        assert_eq!(nix_package_for("pnpm"), Some("nixpkgs#pnpm"));
        assert_eq!(nix_package_for("corepack"), Some("nixpkgs#nodejs"));
        assert_eq!(nix_package_for("node"), Some("nixpkgs#nodejs"));
    }

    #[test]
    fn nix_package_for_returns_none_for_unmapped_tools() {
        assert_eq!(nix_package_for("nix"), None);
        assert_eq!(nix_package_for("kill"), None);
        assert_eq!(nix_package_for("cargo"), None);
        assert_eq!(nix_package_for("unknown-tool"), None);
    }

    #[test]
    fn nix_run_args_produces_correct_invocation() {
        let args = nix_run_args("nixpkgs#git", "git", &["status"]);
        assert_eq!(args, vec!["run", "nixpkgs#git", "--", "git", "status"]);
    }

    #[test]
    fn nix_run_args_with_no_tool_args() {
        let args = nix_run_args("nixpkgs#tmux", "tmux", &[]);
        assert_eq!(args, vec!["run", "nixpkgs#tmux", "--", "tmux"]);
    }
}
