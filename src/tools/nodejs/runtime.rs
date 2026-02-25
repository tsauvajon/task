use crate::{error::Result, runtime::process::ProcessRunner};

use super::runner::{
    corepack_binary_available, node_binary_available, pnpm_binary_available, run_corepack_status,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Pnpm,
    Corepack,
}

pub fn node_available(_process: ProcessRunner) -> bool {
    node_binary_available()
}

pub fn corepack_available(_process: ProcessRunner) -> bool {
    corepack_binary_available()
}

pub fn enable_corepack(_process: ProcessRunner) -> Result<()> {
    run_corepack_status(&["enable"], None)
}

pub fn resolve_runner(_process: ProcessRunner) -> Option<Runner> {
    if pnpm_binary_available() {
        return Some(Runner::Pnpm);
    }
    if corepack_binary_available() {
        return Some(Runner::Corepack);
    }
    None
}
