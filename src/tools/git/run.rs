use std::path::Path;

use crate::{error::Result, runtime::process};

// The underlying `process::run_*` helpers map `ErrorKind::NotFound` on the
// program lookup into `Error::tool_missing(ExternalTool::Git)` with a Nix
// install hint, so no explicit preflight is needed here.

pub(super) fn capture(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    process::run_capture("git", args, cwd)
}

pub(super) fn status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    process::run_status_quiet("git", args, cwd)
}
