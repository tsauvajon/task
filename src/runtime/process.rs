use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
};

use owo_colors::OwoColorize;

use crate::error::{Error, Result};

#[derive(Default)]
struct LogCapture {
    enabled: bool,
    lines: Vec<String>,
}

static LOG_CAPTURE: OnceLock<Mutex<LogCapture>> = OnceLock::new();

fn log_capture() -> &'static Mutex<LogCapture> {
    LOG_CAPTURE.get_or_init(|| Mutex::new(LogCapture::default()))
}

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

/// Returns true if the named command is available.
///
/// For tools with a nix package mapping, availability is determined by
/// whether `nix` itself is on PATH (the tool will be fetched via `nix run`
/// on demand). For unmapped tools and absolute paths, the binary is looked
/// up directly on PATH / filesystem.
pub fn command_exists(name: &str) -> bool {
    if name.contains('/') {
        return Path::new(name).exists();
    }

    if ManagedTool::from_binary(name).is_some() {
        return nix_available();
    }

    let path_var = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path_var).any(|dir| dir.join(name).exists())
}

/// Returns true if `nix` is available on PATH.
pub fn nix_available() -> bool {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path_var).any(|dir| dir.join("nix").exists())
}

pub fn run_capture(
    program: impl AsRef<OsStr>,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String> {
    let program_str = program.as_ref().to_string_lossy();
    let plan = CommandPlan::from_program(&program_str, args);
    let plan_args = plan.args_refs();
    run_capture_raw(plan.program(), &plan_args, cwd)
}

pub fn run_status(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let program_str = program.as_ref().to_string_lossy();
    let plan = CommandPlan::from_program(&program_str, args);
    let plan_args = plan.args_refs();
    run_status_raw(plan.program(), &plan_args, cwd)
}

pub fn run_status_quiet(
    program: impl AsRef<OsStr>,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<()> {
    let program_str = program.as_ref().to_string_lossy();
    let plan = CommandPlan::from_program(&program_str, args);
    let plan_args = plan.args_refs();
    run_status_quiet_raw(plan.program(), &plan_args, cwd)
}

pub fn spawn_detached(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let program_str = program.as_ref().to_string_lossy();
    let plan = CommandPlan::from_program(&program_str, args);
    let plan_args = plan.args_refs();
    spawn_detached_raw(plan.program(), &plan_args, cwd)
}

fn run_capture_raw(
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

fn run_status_raw(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<()> {
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

fn run_status_quiet_raw(
    program: impl AsRef<OsStr>,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<()> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd.output()?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let msg = if stderr.is_empty() {
        format!("command failed with status {}", output.status)
    } else {
        stderr
    };
    Err(Error::failed(msg))
}

fn spawn_detached_raw(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.spawn().map(|_| ()).map_err(Error::from)
}

pub fn log(message: &str) {
    if capture_log_line(&format!("==> {message}")) {
        return;
    }
    println!("{} {}", "==>".bright_blue().bold(), message);
}

pub fn warn(message: &str) {
    if capture_log_line(&format!("warning: {message}")) {
        return;
    }
    eprintln!("{} {}", "warning:".yellow().bold(), message);
}

pub fn enable_log_capture() {
    if let Ok(mut capture) = log_capture().lock() {
        capture.enabled = true;
        capture.lines.clear();
    }
}

pub fn disable_log_capture() {
    if let Ok(mut capture) = log_capture().lock() {
        capture.enabled = false;
        capture.lines.clear();
    }
}

pub fn take_captured_logs() -> Vec<String> {
    if let Ok(mut capture) = log_capture().lock() {
        return std::mem::take(&mut capture.lines);
    }
    Vec::new()
}

fn capture_log_line(line: &str) -> bool {
    if let Ok(mut capture) = log_capture().lock()
        && capture.enabled
    {
        capture.lines.push(line.to_string());
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{CommandPlan, ManagedTool, command_exists};

    mod managed_tool {
        use super::*;

        #[test]
        fn from_binary_maps_known_tools() {
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
        fn from_binary_returns_none_for_unmapped_tools() {
            assert_eq!(ManagedTool::from_binary("nix"), None);
            assert_eq!(ManagedTool::from_binary("kill"), None);
            assert_eq!(ManagedTool::from_binary("cargo"), None);
            assert_eq!(ManagedTool::from_binary("unknown-tool"), None);
        }
    }

    mod command_plan {
        use super::*;

        #[test]
        fn wraps_managed_tool_with_nix_run() {
            let plan = CommandPlan::from_program("git", &["status"]);
            assert_eq!(plan.program(), "nix");
            assert_eq!(plan.args(), vec!["run", "nixpkgs#git", "--", "status"]);
        }

        #[test]
        fn keeps_direct_programs_unwrapped() {
            let plan = CommandPlan::from_program("kill", &["-TERM", "123"]);
            assert_eq!(plan.program(), "kill");
            assert_eq!(plan.args(), vec!["-TERM", "123"]);
        }

        #[test]
        fn args_refs_returns_str_slices() {
            let plan = CommandPlan::from_program("kill", &["-TERM", "456"]);
            assert_eq!(plan.args_refs(), vec!["-TERM", "456"]);
        }

        #[test]
        fn for_managed_tool_with_extra_args_appends_after_separator() {
            let plan = CommandPlan::for_managed_tool(
                ManagedTool::Git,
                vec!["status".to_string(), "--short".to_string()],
            );
            assert_eq!(plan.program(), "nix");
            assert_eq!(
                plan.args(),
                vec!["run", "nixpkgs#git", "--", "status", "--short"]
            );
        }

        #[test]
        fn for_managed_tool_with_no_extra_args_ends_with_separator() {
            let plan = CommandPlan::for_managed_tool(ManagedTool::Tmux, Vec::new());
            assert_eq!(plan.program(), "nix");
            assert_eq!(plan.args(), vec!["run", "nixpkgs#tmux", "--"]);
        }
    }

    mod managed_tool_metadata {
        use super::*;

        #[test]
        fn display_returns_binary_name() {
            assert_eq!(ManagedTool::Git.to_string(), "git");
            assert_eq!(ManagedTool::Tmux.to_string(), "tmux");
            assert_eq!(ManagedTool::Corepack.to_string(), "corepack");
        }

        #[test]
        fn nix_package_returns_expected_package() {
            assert_eq!(ManagedTool::Git.nix_package(), "nixpkgs#git");
            assert_eq!(ManagedTool::Node.nix_package(), "nixpkgs#nodejs");
            assert_eq!(ManagedTool::Corepack.nix_package(), "nixpkgs#nodejs");
        }

        #[test]
        fn binary_name_matches_tool_name() {
            assert_eq!(ManagedTool::Git.binary_name(), "git");
            assert_eq!(ManagedTool::Opencode.binary_name(), "opencode");
            assert_eq!(ManagedTool::Pnpm.binary_name(), "pnpm");
        }

        #[test]
        fn all_tools_have_non_empty_nix_package() {
            let tools = [
                ManagedTool::Git,
                ManagedTool::Tmux,
                ManagedTool::Codium,
                ManagedTool::Opencode,
                ManagedTool::Direnv,
                ManagedTool::Asdf,
                ManagedTool::Pnpm,
                ManagedTool::Corepack,
                ManagedTool::Node,
            ];
            for tool in &tools {
                assert!(
                    !tool.nix_package().is_empty(),
                    "{tool} has empty nix_package"
                );
                assert!(
                    !tool.binary_name().is_empty(),
                    "{tool} has empty binary_name"
                );
            }
        }

        #[test]
        fn managed_tool_is_copy_and_eq() {
            let a = ManagedTool::Git;
            let b = a; // Copy
            assert_eq!(a, b);
        }

        #[test]
        fn direnv_nix_package() {
            assert_eq!(ManagedTool::Direnv.nix_package(), "nixpkgs#direnv");
            assert_eq!(ManagedTool::Direnv.binary_name(), "direnv");
        }

        #[test]
        fn asdf_nix_package() {
            assert_eq!(ManagedTool::Asdf.nix_package(), "nixpkgs#asdf-vm");
            assert_eq!(ManagedTool::Asdf.binary_name(), "asdf");
        }

        #[test]
        fn codium_nix_package() {
            assert_eq!(ManagedTool::Codium.nix_package(), "nixpkgs#vscodium");
            assert_eq!(ManagedTool::Codium.binary_name(), "codium");
        }
    }

    mod command_exists {
        use super::*;

        #[test]
        fn returns_true_for_known_system_binary() {
            // `true` is universally available on POSIX systems
            assert!(command_exists("true"));
        }

        #[test]
        fn returns_false_for_unknown_binary() {
            assert!(!command_exists("this-binary-should-not-exist-xyz-12345"));
        }

        #[test]
        fn returns_true_for_existing_absolute_path() {
            // /usr/bin/env is available on macOS and Linux
            assert!(command_exists("/usr/bin/env"));
        }

        #[test]
        fn returns_false_for_nonexistent_absolute_path() {
            assert!(!command_exists("/this/path/does/not/exist/xyz"));
        }

        #[test]
        fn slash_in_name_triggers_filesystem_check() {
            // A relative path that contains a slash but doesn't exist
            assert!(!command_exists("relative/path/to/nothing"));
        }
    }
}
