use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::{
    error::Result,
    runtime::{
        nix_store::{cached_nix_binary, run_nix_binary_capture, run_nix_binary_status},
        process::ManagedTool,
    },
};

static NIX_GIT_BINARY: OnceLock<Result<PathBuf>> = OnceLock::new();

pub(super) fn run_git_capture(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let binary = cached_nix_binary(&NIX_GIT_BINARY, ManagedTool::Git)?;
    run_nix_binary_capture(binary, args, cwd)
}

pub(super) fn run_git_status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let binary = cached_nix_binary(&NIX_GIT_BINARY, ManagedTool::Git)?;
    run_nix_binary_status(binary, args, cwd)
}

#[cfg(test)]
mod tests {
    // Integration with the real Nix store is tested at the tool level.
    // The shared parse / resolve logic is tested in runtime::nix_store.
}
