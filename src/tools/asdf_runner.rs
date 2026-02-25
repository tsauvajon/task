use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::runtime::{
    nix_store::{cached_nix_binary, run_nix_binary_capture, run_nix_binary_status},
    process::ManagedTool,
};

static NIX_ASDF_BINARY: OnceLock<Result<PathBuf, String>> = OnceLock::new();

fn asdf_binary() -> Result<&'static PathBuf, String> {
    cached_nix_binary(&NIX_ASDF_BINARY, ManagedTool::Asdf)
}

pub fn run_asdf_capture(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let binary = asdf_binary()?;
    run_nix_binary_capture(binary, args, cwd)
}

pub fn run_asdf_status(args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
    let binary = asdf_binary()?;
    run_nix_binary_status(binary, args, cwd)
}
