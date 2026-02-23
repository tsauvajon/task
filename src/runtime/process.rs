use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use owo_colors::OwoColorize;

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessRunner;

impl ProcessRunner {
    pub fn command_exists(&self, name: &str) -> bool {
        if name.contains('/') {
            return Path::new(name).exists();
        }

        let path_var = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path_var).any(|dir| dir.join(name).exists())
    }

    pub fn run_capture(
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

    pub fn run_status(
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

    pub fn tmux_has_session(&self, session: &str) -> bool {
        match Command::new("tmux")
            .args(["has-session", "-t", session])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    pub fn log(&self, message: &str) {
        println!("{} {}", "==>".bright_blue().bold(), message);
    }

    pub fn warn(&self, message: &str) {
        eprintln!("{} {}", "warning:".yellow().bold(), message);
    }
}
