use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::runtime::{
    nix_store::{cached_nix_binary, run_nix_binary_status},
    process::ManagedTool,
};

static NIX_DIRENV_BINARY: OnceLock<Result<PathBuf, String>> = OnceLock::new();

fn direnv_binary() -> Result<&'static PathBuf, String> {
    cached_nix_binary(&NIX_DIRENV_BINARY, ManagedTool::Direnv)
}

pub fn run_direnv_status(args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
    let binary = direnv_binary()?;
    run_nix_binary_status(binary, args, cwd)
}
