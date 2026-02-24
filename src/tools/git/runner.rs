use std::{ffi::OsStr, path::Path, process::Command};

pub(super) fn run_git_capture(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    run_capture("git", args, cwd)
}

pub(super) fn run_git_status(args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
    run_status("git", args, cwd)
}

fn run_capture(
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

fn run_status(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
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
