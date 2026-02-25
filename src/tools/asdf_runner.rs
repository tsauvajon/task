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

static NIX_ASDF_BINARY: OnceLock<Result<PathBuf>> = OnceLock::new();

fn asdf_binary() -> Result<&'static PathBuf> {
    cached_nix_binary(&NIX_ASDF_BINARY, ManagedTool::Asdf)
}

pub fn run_asdf_capture(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    run_nix_binary_capture(asdf_binary()?, args, cwd)
}

pub fn run_asdf_status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    run_nix_binary_status(asdf_binary()?, args, cwd)
}
