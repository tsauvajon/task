use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::{
    error::Result,
    runtime::{
        nix_store::{cached_nix_binary, run_nix_binary_status},
        process::ManagedTool,
    },
};

// pnpm and corepack ship from the same nixpkgs package (nodejs), but we
// resolve each binary separately so the OnceLock stores the right path.
static NIX_PNPM_BINARY: OnceLock<Result<PathBuf>> = OnceLock::new();
static NIX_COREPACK_BINARY: OnceLock<Result<PathBuf>> = OnceLock::new();
static NIX_NODE_BINARY: OnceLock<Result<PathBuf>> = OnceLock::new();

fn pnpm_binary() -> Result<&'static PathBuf> {
    cached_nix_binary(&NIX_PNPM_BINARY, ManagedTool::Pnpm)
}

fn corepack_binary() -> Result<&'static PathBuf> {
    cached_nix_binary(&NIX_COREPACK_BINARY, ManagedTool::Corepack)
}

pub fn run_pnpm_status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    run_nix_binary_status(pnpm_binary()?, args, cwd)
}

pub fn run_corepack_status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    run_nix_binary_status(corepack_binary()?, args, cwd)
}

/// Returns `true` if the `node` binary can be resolved in the Nix store.
pub fn node_binary_available() -> bool {
    cached_nix_binary(&NIX_NODE_BINARY, ManagedTool::Node).is_ok()
}

/// Returns `true` if the `pnpm` binary can be resolved in the Nix store.
pub fn pnpm_binary_available() -> bool {
    pnpm_binary().is_ok()
}

/// Returns `true` if the `corepack` binary can be resolved in the Nix store.
pub fn corepack_binary_available() -> bool {
    corepack_binary().is_ok()
}
