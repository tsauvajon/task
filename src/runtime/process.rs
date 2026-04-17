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

/// External binaries the CLI shells out to. The enum captures just enough
/// metadata to generate install hints when the binary is missing from PATH.
///
/// Previously `ManagedTool` — renamed because we no longer resolve or launch
/// these via Nix at runtime. We just know their names and how the user can
/// install them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalTool {
    Git,
    Tmux,
    Codium,
    Opencode,
    Direnv,
    Asdf,
    Pnpm,
    Corepack,
    Node,
    Cargo,
    Nix,
}

/// Install guidance attached to an [`ExternalTool`]. Most tools ship in
/// `nixpkgs`; a few (like `nix` itself) need a different channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallHint {
    /// `nix profile install <package>` (e.g. `nixpkgs#git`).
    NixPackage(&'static str),
    /// Free-form hint, used when the tool can't be installed via
    /// `nix profile install` (or has a strongly-preferred channel).
    Custom(&'static str),
}

impl std::fmt::Display for InstallHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NixPackage(pkg) => write!(f, "nix profile install {pkg}"),
            Self::Custom(msg) => f.write_str(msg),
        }
    }
}

impl ExternalTool {
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
            "cargo" => Some(Self::Cargo),
            "nix" => Some(Self::Nix),
            _ => None,
        }
    }

    pub fn install_hint(self) -> InstallHint {
        match self {
            Self::Git => InstallHint::NixPackage("nixpkgs#git"),
            Self::Tmux => InstallHint::NixPackage("nixpkgs#tmux"),
            Self::Codium => InstallHint::NixPackage("nixpkgs#vscodium"),
            Self::Opencode => InstallHint::NixPackage("nixpkgs#opencode"),
            Self::Direnv => InstallHint::NixPackage("nixpkgs#direnv"),
            Self::Asdf => InstallHint::NixPackage("nixpkgs#asdf-vm"),
            Self::Pnpm => InstallHint::NixPackage("nixpkgs#pnpm"),
            Self::Corepack | Self::Node => InstallHint::NixPackage("nixpkgs#nodejs"),
            Self::Cargo => InstallHint::Custom(
                "install via rustup (https://rustup.rs) or nix profile install nixpkgs#cargo",
            ),
            Self::Nix => InstallHint::Custom("see https://nixos.org/download"),
        }
    }

    /// The binary name used to look the tool up on PATH.
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
            Self::Cargo => "cargo",
            Self::Nix => "nix",
        }
    }

    /// All external tools, in a stable order suitable for doctor output.
    pub fn all() -> &'static [ExternalTool] {
        &[
            Self::Nix,
            Self::Git,
            Self::Tmux,
            Self::Opencode,
            Self::Codium,
            Self::Direnv,
            Self::Asdf,
            Self::Node,
            Self::Corepack,
            Self::Pnpm,
            Self::Cargo,
        ]
    }
}

impl std::fmt::Display for ExternalTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.binary_name())
    }
}

/// A prepared program + arguments pair. Used when the invocation needs to be
/// serialized (e.g. passed through `tmux new-session` as a shell command),
/// as well as inside the generic `run_*` helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    program: String,
    args: Vec<String>,
}

impl CommandPlan {
    pub fn from_program(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|&a| a.to_string()).collect(),
        }
    }

    /// Build a plan invoking a known external tool directly from PATH.
    pub fn for_tool(tool: ExternalTool, tool_args: Vec<String>) -> Self {
        Self {
            program: tool.binary_name().to_string(),
            args: tool_args,
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

/// Returns true if the named binary is reachable via PATH (or exists as an
/// absolute path).
pub fn command_exists(name: &str) -> bool {
    if name.contains('/') {
        return Path::new(name).exists();
    }

    let path_var = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path_var).any(|dir| dir.join(name).exists())
}

/// Maps an `io::Error` from spawning a process into a user-friendlier error.
///
/// When the OS reports "No such file or directory" (`ErrorKind::NotFound`) for
/// the program itself and the program name corresponds to a known
/// [`ExternalTool`], return [`Error::tool_missing`] so the user sees the
/// install hint instead of a bare syscall message. For unknown programs, the
/// message still says "not found on PATH" (instead of raw `io::Error`).
fn spawn_error(program: &OsStr, err: std::io::Error) -> Error {
    if err.kind() != std::io::ErrorKind::NotFound {
        return Error::from(err);
    }
    let Some(name) = program.to_str() else {
        return Error::from(err);
    };
    if let Some(tool) = ExternalTool::from_binary(name) {
        return Error::tool_missing(tool);
    }
    Error::failed(format!("Program `{name}` not found on PATH"))
}

pub fn run_capture(
    program: impl AsRef<OsStr>,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String> {
    let program = program.as_ref();
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd.output().map_err(|e| spawn_error(program, e))?;
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

pub fn run_status(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let program = program.as_ref();
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let status = cmd.status().map_err(|e| spawn_error(program, e))?;
    if status.success() {
        return Ok(());
    }
    Err(Error::failed(format!(
        "command failed with status {status}"
    )))
}

pub fn run_status_quiet(
    program: impl AsRef<OsStr>,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<()> {
    let program = program.as_ref();
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd.output().map_err(|e| spawn_error(program, e))?;
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

pub fn spawn_detached(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let program = program.as_ref();
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.spawn().map(|_| ()).map_err(|e| spawn_error(program, e))
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
    use std::ffi::OsStr;

    use super::{CommandPlan, ExternalTool, InstallHint, command_exists, spawn_error};
    use crate::error::Error;

    mod external_tool {
        use super::*;

        #[test]
        fn from_binary_maps_known_tools() {
            assert_eq!(ExternalTool::from_binary("git"), Some(ExternalTool::Git));
            assert_eq!(ExternalTool::from_binary("tmux"), Some(ExternalTool::Tmux));
            assert_eq!(
                ExternalTool::from_binary("codium"),
                Some(ExternalTool::Codium)
            );
            assert_eq!(
                ExternalTool::from_binary("opencode"),
                Some(ExternalTool::Opencode)
            );
            assert_eq!(
                ExternalTool::from_binary("direnv"),
                Some(ExternalTool::Direnv)
            );
            assert_eq!(ExternalTool::from_binary("asdf"), Some(ExternalTool::Asdf));
            assert_eq!(ExternalTool::from_binary("pnpm"), Some(ExternalTool::Pnpm));
            assert_eq!(
                ExternalTool::from_binary("corepack"),
                Some(ExternalTool::Corepack)
            );
            assert_eq!(ExternalTool::from_binary("node"), Some(ExternalTool::Node));
            assert_eq!(
                ExternalTool::from_binary("cargo"),
                Some(ExternalTool::Cargo)
            );
            assert_eq!(ExternalTool::from_binary("nix"), Some(ExternalTool::Nix));
        }

        #[test]
        fn from_binary_returns_none_for_unmapped_tools() {
            assert_eq!(ExternalTool::from_binary("kill"), None);
            assert_eq!(ExternalTool::from_binary("rustfmt"), None);
            assert_eq!(ExternalTool::from_binary("unknown-tool"), None);
        }
    }

    mod command_plan {
        use super::*;

        #[test]
        fn from_program_keeps_program_and_args_direct() {
            let plan = CommandPlan::from_program("git", &["status"]);
            assert_eq!(plan.program(), "git");
            assert_eq!(plan.args(), vec!["status"]);
        }

        #[test]
        fn from_program_keeps_unknown_tools_direct_too() {
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
        fn for_tool_resolves_to_binary_name_on_path() {
            let plan = CommandPlan::for_tool(
                ExternalTool::Git,
                vec!["status".to_string(), "--short".to_string()],
            );
            assert_eq!(plan.program(), "git");
            assert_eq!(plan.args(), vec!["status", "--short"]);
        }

        #[test]
        fn for_tool_with_no_extra_args_has_empty_args() {
            let plan = CommandPlan::for_tool(ExternalTool::Tmux, Vec::new());
            assert_eq!(plan.program(), "tmux");
            assert!(plan.args().is_empty());
        }
    }

    mod external_tool_metadata {
        use super::*;

        #[test]
        fn display_returns_binary_name() {
            assert_eq!(ExternalTool::Git.to_string(), "git");
            assert_eq!(ExternalTool::Tmux.to_string(), "tmux");
            assert_eq!(ExternalTool::Corepack.to_string(), "corepack");
            assert_eq!(ExternalTool::Cargo.to_string(), "cargo");
            assert_eq!(ExternalTool::Nix.to_string(), "nix");
        }

        #[test]
        fn install_hint_uses_nix_package_for_nixpkgs_tools() {
            assert_eq!(
                ExternalTool::Git.install_hint(),
                InstallHint::NixPackage("nixpkgs#git")
            );
            assert_eq!(
                ExternalTool::Node.install_hint(),
                InstallHint::NixPackage("nixpkgs#nodejs")
            );
            assert_eq!(
                ExternalTool::Corepack.install_hint(),
                InstallHint::NixPackage("nixpkgs#nodejs")
            );
        }

        #[test]
        fn install_hint_for_cargo_mentions_rustup_and_nix() {
            let hint = ExternalTool::Cargo.install_hint();
            assert!(matches!(hint, InstallHint::Custom(_)));
            let msg = hint.to_string();
            assert!(
                msg.contains("rustup"),
                "cargo hint should mention rustup: {msg}"
            );
            assert!(msg.contains("nix"), "cargo hint should mention nix: {msg}");
        }

        #[test]
        fn install_hint_for_nix_points_at_nixos_download() {
            let hint = ExternalTool::Nix.install_hint();
            assert!(matches!(hint, InstallHint::Custom(_)));
            assert!(hint.to_string().contains("nixos.org/download"));
        }

        #[test]
        fn nix_package_hint_renders_as_nix_profile_command() {
            let rendered = InstallHint::NixPackage("nixpkgs#git").to_string();
            assert_eq!(rendered, "nix profile install nixpkgs#git");
        }

        #[test]
        fn binary_name_matches_tool_name() {
            assert_eq!(ExternalTool::Git.binary_name(), "git");
            assert_eq!(ExternalTool::Opencode.binary_name(), "opencode");
            assert_eq!(ExternalTool::Pnpm.binary_name(), "pnpm");
            assert_eq!(ExternalTool::Cargo.binary_name(), "cargo");
            assert_eq!(ExternalTool::Nix.binary_name(), "nix");
        }

        #[test]
        fn all_tools_have_non_empty_metadata() {
            for tool in ExternalTool::all() {
                assert!(
                    !tool.install_hint().to_string().is_empty(),
                    "{tool} has empty install hint"
                );
                assert!(
                    !tool.binary_name().is_empty(),
                    "{tool} has empty binary_name"
                );
            }
        }

        #[test]
        fn external_tool_is_copy_and_eq() {
            let a = ExternalTool::Git;
            let b = a; // Copy
            assert_eq!(a, b);
        }

        #[test]
        fn all_contains_every_variant() {
            let all = ExternalTool::all();
            assert!(all.contains(&ExternalTool::Git));
            assert!(all.contains(&ExternalTool::Tmux));
            assert!(all.contains(&ExternalTool::Codium));
            assert!(all.contains(&ExternalTool::Opencode));
            assert!(all.contains(&ExternalTool::Direnv));
            assert!(all.contains(&ExternalTool::Asdf));
            assert!(all.contains(&ExternalTool::Pnpm));
            assert!(all.contains(&ExternalTool::Corepack));
            assert!(all.contains(&ExternalTool::Node));
            assert!(all.contains(&ExternalTool::Cargo));
            assert!(all.contains(&ExternalTool::Nix));
        }
    }

    mod spawn_error_mapping {
        use super::*;

        fn not_found_io_error() -> std::io::Error {
            std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory")
        }

        #[test]
        fn maps_not_found_for_known_tool_to_tool_missing_hint() {
            let err = spawn_error(OsStr::new("git"), not_found_io_error());
            let msg = err.to_string();
            assert!(matches!(err, Error::Failed(_)));
            assert!(
                msg.contains("`git`") && msg.contains("nix profile install nixpkgs#git"),
                "expected tool_missing hint, got: {msg}"
            );
        }

        #[test]
        fn maps_not_found_for_cargo_to_cargo_specific_hint() {
            let err = spawn_error(OsStr::new("cargo"), not_found_io_error());
            let msg = err.to_string();
            assert!(matches!(err, Error::Failed(_)));
            assert!(msg.contains("`cargo`"), "should quote cargo: {msg}");
            assert!(
                msg.contains("rustup"),
                "cargo hint should mention rustup: {msg}"
            );
        }

        #[test]
        fn maps_not_found_for_unknown_binary_to_generic_message() {
            let err = spawn_error(OsStr::new("not-a-known-tool"), not_found_io_error());
            let msg = err.to_string();
            assert!(matches!(err, Error::Failed(_)));
            assert!(
                msg.contains("`not-a-known-tool`") && msg.contains("not found on PATH"),
                "expected generic not-found message, got: {msg}"
            );
        }

        #[test]
        fn passes_through_non_not_found_errors_for_known_tools() {
            let io_err = std::io::Error::other("something else");
            let err = spawn_error(OsStr::new("git"), io_err);
            assert!(matches!(err, Error::Io(_)));
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
