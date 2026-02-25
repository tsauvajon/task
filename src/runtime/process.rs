use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Stdio},
};

use owo_colors::OwoColorize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedTool {
    Git,
    Tmux,
    Codium,
    Opencode,
    Direnv,
    Asdf,
    Pnpm,
    Corepack,
    Node,
}

impl ManagedTool {
    pub fn from_binary(binary: &str) -> Option<Self> {
        match binary {
            "git" => Some(Self::Git),
            "tmux" => Some(Self::Tmux),
            "codium" => Some(Self::Codium),
            "opencode" => Some(Self::Opencode),
            "direnv" => Some(Self::Direnv),
            "asdf" => Some(Self::Asdf),
            "pnpm" => Some(Self::Pnpm),
            "corepack" => Some(Self::Corepack),
            "node" => Some(Self::Node),
            _ => None,
        }
    }

    pub fn nix_package(self) -> &'static str {
        match self {
            Self::Git => "nixpkgs#git",
            Self::Tmux => "nixpkgs#tmux",
            Self::Codium => "nixpkgs#vscodium",
            Self::Opencode => "nixpkgs#opencode",
            Self::Direnv => "nixpkgs#direnv",
            Self::Asdf => "nixpkgs#asdf-vm",
            Self::Pnpm => "nixpkgs#pnpm",
            Self::Corepack | Self::Node => "nixpkgs#nodejs",
        }
    }

    /// The binary name exposed inside the Nix package's `bin/` directory.
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::Tmux => "tmux",
            Self::Codium => "codium",
            Self::Opencode => "opencode",
            Self::Direnv => "direnv",
            Self::Asdf => "asdf",
            Self::Pnpm => "pnpm",
            Self::Corepack => "corepack",
            Self::Node => "node",
        }
    }
}

impl std::fmt::Display for ManagedTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.binary_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    program: String,
    args: Vec<String>,
}

impl CommandPlan {
    pub fn from_program(program: &str, args: &[&str]) -> Self {
        let args: Vec<String> = args.iter().map(|&a| a.to_string()).collect();
        if let Some(tool) = ManagedTool::from_binary(program) {
            return Self::for_managed_tool(tool, args);
        }
        Self {
            program: program.to_string(),
            args,
        }
    }

    pub fn for_managed_tool(tool: ManagedTool, tool_args: Vec<String>) -> Self {
        let mut args = vec![
            "run".to_string(),
            tool.nix_package().to_string(),
            "--".to_string(),
        ];
        args.extend(tool_args);
        Self {
            program: "nix".to_string(),
            args,
        }
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn args_refs(&self) -> Vec<&str> {
        self.args.iter().map(String::as_str).collect()
    }
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

        if ManagedTool::from_binary(name).is_some() {
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
    ) -> Result<String> {
        let program_str = program.as_ref().to_string_lossy();
        let plan = CommandPlan::from_program(&program_str, args);
        let plan_args = plan.args_refs();
        self.run_capture_raw(plan.program(), &plan_args, cwd)
    }

    pub fn run_status(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<()> {
        let program_str = program.as_ref().to_string_lossy();
        let plan = CommandPlan::from_program(&program_str, args);
        let plan_args = plan.args_refs();
        self.run_status_raw(plan.program(), &plan_args, cwd)
    }

    pub fn spawn_detached(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<()> {
        let program_str = program.as_ref().to_string_lossy();
        let plan = CommandPlan::from_program(&program_str, args);
        let plan_args = plan.args_refs();
        self.spawn_detached_raw(plan.program(), &plan_args, cwd)
    }

    fn run_capture_raw(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<String> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let msg = if stderr.is_empty() {
                format!("command failed with status {}", output.status)
            } else {
                stderr
            };
            return Err(Error::failed(msg));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn run_status_raw(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<()> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        let status = cmd.status()?;
        if status.success() {
            return Ok(());
        }
        Err(Error::failed(format!(
            "command failed with status {status}"
        )))
    }

    fn spawn_detached_raw(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<()> {
        let mut cmd = Command::new(program);
        cmd.args(args).stdout(Stdio::null()).stderr(Stdio::null());
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        cmd.spawn().map(|_| ()).map_err(Error::from)
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
    use super::{CommandPlan, ManagedTool};

    #[test]
    fn managed_tool_from_binary_maps_known_tools() {
        assert_eq!(ManagedTool::from_binary("git"), Some(ManagedTool::Git));
        assert_eq!(ManagedTool::from_binary("tmux"), Some(ManagedTool::Tmux));
        assert_eq!(
            ManagedTool::from_binary("codium"),
            Some(ManagedTool::Codium)
        );
        assert_eq!(
            ManagedTool::from_binary("opencode"),
            Some(ManagedTool::Opencode)
        );
        assert_eq!(
            ManagedTool::from_binary("direnv"),
            Some(ManagedTool::Direnv)
        );
        assert_eq!(ManagedTool::from_binary("asdf"), Some(ManagedTool::Asdf));
        assert_eq!(ManagedTool::from_binary("pnpm"), Some(ManagedTool::Pnpm));
        assert_eq!(
            ManagedTool::from_binary("corepack"),
            Some(ManagedTool::Corepack)
        );
        assert_eq!(ManagedTool::from_binary("node"), Some(ManagedTool::Node));
    }

    #[test]
    fn managed_tool_from_binary_returns_none_for_unmapped_tools() {
        assert_eq!(ManagedTool::from_binary("nix"), None);
        assert_eq!(ManagedTool::from_binary("kill"), None);
        assert_eq!(ManagedTool::from_binary("cargo"), None);
        assert_eq!(ManagedTool::from_binary("unknown-tool"), None);
    }

    #[test]
    fn command_plan_wraps_managed_tool_with_nix_run() {
        let plan = CommandPlan::from_program("git", &["status"]);
        assert_eq!(plan.program(), "nix");
        assert_eq!(plan.args(), vec!["run", "nixpkgs#git", "--", "status"]);
    }

    #[test]
    fn command_plan_keeps_direct_programs_unwrapped() {
        let plan = CommandPlan::from_program("kill", &["-TERM", "123"]);
        assert_eq!(plan.program(), "kill");
        assert_eq!(plan.args(), vec!["-TERM", "123"]);
    }
}
