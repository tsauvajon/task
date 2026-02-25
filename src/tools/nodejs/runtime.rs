use std::path::Path;

use crate::{
    error::Result,
    runtime::{nix_store::NixRunner, process::ManagedTool},
};

// pnpm and corepack ship from the same nixpkgs package (nodejs), but we
// resolve each binary separately so the NixRunner stores the right path.
static PNPM: NixRunner = NixRunner::new(ManagedTool::Pnpm);
static COREPACK: NixRunner = NixRunner::new(ManagedTool::Corepack);
static NODE: NixRunner = NixRunner::new(ManagedTool::Node);

pub fn pnpm_status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    PNPM.status(args, cwd)
}

pub fn corepack_status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    COREPACK.status(args, cwd)
}

/// Returns `true` if the `node` binary can be resolved in the Nix store.
pub fn node_binary_available() -> bool {
    NODE.available()
}

/// Returns `true` if the `pnpm` binary can be resolved in the Nix store.
pub fn pnpm_binary_available() -> bool {
    PNPM.available()
}

/// Returns `true` if the `corepack` binary can be resolved in the Nix store.
pub fn corepack_binary_available() -> bool {
    COREPACK.available()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Pnpm,
    Corepack,
}

pub fn node_available() -> bool {
    node_binary_available()
}

pub fn corepack_available() -> bool {
    corepack_binary_available()
}

pub fn enable_corepack() -> Result<()> {
    corepack_status(&["enable"], None)
}

pub fn resolve_runner() -> Option<Runner> {
    if pnpm_binary_available() {
        return Some(Runner::Pnpm);
    }
    if corepack_binary_available() {
        return Some(Runner::Corepack);
    }
    None
}
