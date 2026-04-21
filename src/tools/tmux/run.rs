use std::path::Path;

use crate::runtime::process;

pub(super) fn available() -> bool {
    process::command_exists("tmux")
}

pub(super) fn capture(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<String> {
    process::run_capture("tmux", args, cwd)
}

pub(super) fn status(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<()> {
    process::run_status("tmux", args, cwd)
}

pub(super) fn status_quiet(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<()> {
    process::run_status_quiet("tmux", args, cwd)
}
