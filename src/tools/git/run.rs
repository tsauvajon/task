use std::path::Path;

use crate::runtime::{nix_store::NixRunner, process::ManagedTool};

static GIT: NixRunner = NixRunner::new(ManagedTool::Git);

/// Eagerly resolve the nix store path for git, so the first real git call
/// does not pay the `nix path-info` cost (~0.5s) inside a parallel section
/// where all other threads would stall waiting on the `OnceLock`.
pub fn warmup() {
    let _ = GIT.available();
}

pub(super) fn capture(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<String> {
    GIT.capture(args, cwd)
}

pub(super) fn status(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<()> {
    GIT.status(args, cwd)
}
