use std::path::Path;

use crate::runtime::{nix_store::NixRunner, process::ManagedTool};

static GIT: NixRunner = NixRunner::new(ManagedTool::Git);

pub(super) fn run_git_capture(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<String> {
    GIT.capture(args, cwd)
}

pub(super) fn run_git_status(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<()> {
    GIT.status(args, cwd)
}
