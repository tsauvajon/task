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

static NIX_TMUX_BINARY: OnceLock<Result<PathBuf>> = OnceLock::new();

fn tmux_binary() -> Result<&'static PathBuf> {
    cached_nix_binary(&NIX_TMUX_BINARY, ManagedTool::Tmux)
}

pub(super) fn run_tmux_capture(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    run_nix_binary_capture(tmux_binary()?, args, cwd)
}

pub(super) fn run_tmux_status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    run_nix_binary_status(tmux_binary()?, args, cwd)
}
