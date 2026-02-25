use std::path::Path;

use crate::runtime::process::ProcessRunner;

pub(super) fn run_git_capture(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    ProcessRunner.run_capture("git", args, cwd)
}

pub(super) fn run_git_status(args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
    ProcessRunner.run_status("git", args, cwd)
}
